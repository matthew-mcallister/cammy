//! Solves the "Cammy the Camel" problem: Cammy lives D miles from the market
//! and has B bananas she wishes to sell. She must eat one banana per mile
//! traveled. She can carry at most C bananas at a time, but she can leave
//! banana piles anywhere she likes on the road and pick them up later. How
//! many bananas can she sell?
//!
//! We solve this problem using parallel map-reduce.

#![feature(iter_macro, yield_expr)]

use std::borrow::Borrow;
use std::collections::hash_map::Entry;
use std::hash::{BuildHasher, Hash};
use std::iter::iter;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::sync::mpsc::{Receiver, Sender, channel};

use fnv::{FnvBuildHasher, FnvHashMap};

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
struct StateKey<const D: usize> {
    x: u16,
    piles: [u16; D],
}

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
struct State<const D: usize> {
    inner: StateKey<D>,
    held: u16,
}

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
struct StateMeta<const D: usize> {
    prev: Option<State<D>>,
    held: u16,
}

impl<const D: usize> State<D> {
    fn moves(&self) -> impl Iterator<Item = State<D>> + '_ {
        let state = self;
        let it = iter!(move || {
            if state.held == 0 { return; }
            if state.inner.x + 1 < D as u16 {
                State {
                    inner: StateKey {
                        x: state.inner.x + 1,
                        ..state.inner
                    },
                    held: state.held - 1,
                }.yield
            }
            if state.inner.x > 0 {
                State {
                    inner: StateKey {
                        x: state.inner.x - 1,
                        ..state.inner
                    },
                    held: state.held - 1,
                }.yield
            }
        });
        it()
    }

    fn pickup<const C: u16>(&mut self) {
        let state = self;
        let pile = state.inner.x as usize;
        let pickup = std::cmp::min(C - state.held, state.inner.piles[pile]);
        state.held += pickup;
        state.inner.piles[pile] -= pickup;
    }

    fn successors<const C: u16>(&self) -> impl Iterator<Item = State<D>> + '_ {
        let state = self;
        let it = iter!(move || {
            for mut state in state.moves() {
                state.pickup::<C>();
                for amount in 0..=state.held {
                    let mut state = state;
                    state.held -= amount;
                    state.inner.piles[state.inner.x as usize] += amount;
                    state.yield;
                }
            }
        });
        it()
    }
}

#[derive(Debug)]
struct WorkerShared {
    num_workers: u64,
    progress: Mutex<u64>,
    cond_var: Condvar,
    build_hasher: FnvBuildHasher,
    stop_votes: AtomicU64,
    cleanup: AtomicU64,
}

impl WorkerShared {
    fn synchronize(&self, round: u64) {
        let mut progress = self.progress.lock().unwrap();
        *progress += 1;
        let target = (round + 1) * self.num_workers;
        if *progress < target {
            while *progress < target {
                progress = self.cond_var.wait(progress).unwrap();
            }
        } else {
            // At this point, every other thread has aquired the mutex at least
            // once, so it is guaranteed that every vote is visible at this
            // point.
            if self.stop_votes.load(Ordering::Relaxed) != self.num_workers {
                self.stop_votes.store(0, Ordering::Relaxed);
            }
            self.cond_var.notify_all();
        }
    }
}

#[derive(Clone, Debug, Default)]
struct States<const D: usize> {
    // Thought: Could prune states where bananas on ground > current total bananas,
    // since we can never revisit that state key
    inner: FnvHashMap<StateKey<D>, StateMeta<D>>,
    bananas_sold: u16,
    solutions: Vec<StateKey<D>>,
}

impl<const D: usize> States<D> {
    fn insert(&mut self, prev: Option<State<D>>, state: State<D>) -> bool {
        let key = state.inner;
        let meta = StateMeta {
            prev,
            held: state.held,
        };
        let inserted = match self.inner.entry(key) {
            Entry::Occupied(e) => {
                // Because we traverse in BF order, we always visit states with
                // highest held value first
                assert!(e.get().held >= state.held);
                false
            }
            Entry::Vacant(e) => {
                e.insert(meta);
                true
            }
        };
        if state.inner.x == (D - 1) as u16 {
            if state.held > self.bananas_sold {
                self.bananas_sold = state.held;
                self.solutions.clear();
                self.solutions.push(key);
            } else if state.held == self.bananas_sold {
                self.solutions.push(key);
            }
        }
        inserted
    }
}

// Rehashing the entire state space could increase runtime by a constant
// factor, so instead maintain the solution set in its sharded form
#[derive(Clone, Debug, Default)]
struct StateSet<const D: usize> {
    build_hasher: FnvBuildHasher,
    states: Vec<FnvHashMap<StateKey<D>, StateMeta<D>>>,
    bananas_sold: u16,
    solutions: Vec<StateKey<D>>,
}

impl<const D: usize> StateSet<D> {
    fn new(states: impl Iterator<Item = States<D>>) -> Self {
        let mut set = StateSet {
            build_hasher: FnvBuildHasher::default(),
            states: vec![],
            bananas_sold: 0,
            solutions: vec![],
        };
        for state in states {
            set.states.push(state.inner);
            if state.bananas_sold > set.bananas_sold {
                set.bananas_sold = state.bananas_sold;
                set.solutions.clear();
                set.solutions.extend(state.solutions);
            } else if state.bananas_sold == set.bananas_sold {
                set.solutions.extend(state.solutions);
            }
        }
        set
    }

    #[allow(dead_code)]
    fn get<'a>(&'a self, key: impl Borrow<StateKey<D>>) -> Option<&'a StateMeta<D>> {
        let hash = self.build_hasher.hash_one(key.borrow());
        let shard = (hash % (self.states.len() as u64)) as usize;
        self.states[shard].get(key.borrow())
    }
}

type Successors<const D: usize> = Vec<(State<D>, State<D>)>;

#[derive(Debug)]
struct Worker<const D: usize> {
    shared: Arc<WorkerShared>,
    senders: Vec<Sender<Successors<D>>>,
    receiver: Receiver<Successors<D>>,
    answer_sender: Sender<States<D>>,
}

impl<const D: usize> Worker<D> {
    fn run<const C: u16>(self, mut states: States<D>) {
        let worker = self;
        let mut next: Vec<_> = states.inner.iter()
            .map(|(&s, n)| State { inner: s, held: n.held })
            .collect();
        let mut round = 0;
        let debug = std::env::var("DEBUG").is_ok();
        loop {
            if next.is_empty() {
                worker.shared.stop_votes.fetch_add(1, Ordering::Relaxed);
            }

            // Do not send any successors until all successors from previous
            // round have been received
            worker.shared.synchronize(round);

            if worker.shared.stop_votes.load(Ordering::Relaxed) >= worker.shared.num_workers {
                // All workers are done
                worker.answer_sender.send(states).unwrap();

                if worker.shared.cleanup.fetch_add(1, Ordering::Relaxed) + 1
                    >= worker.shared.num_workers
                {
                    println!("finished after {} rounds", round);
                }

                break;
            }

            // Compute successors, sharded on hash
            let mut successors: Vec<Vec<(State<D>, State<D>)>>
                = vec![vec![]; worker.shared.num_workers as usize];
            for state in std::mem::take(&mut next) {
                for successor in state.successors::<C>() {
                    let hash = worker.shared.build_hasher.hash_one(&successor);
                    let shard = (hash % worker.shared.num_workers) as usize;
                    successors[shard].push((state, successor));
                }
            }

            if debug && let Some(s) = successors[0].first() {
                println!(
                    "tid: {:?}, round: {}, successor: {:?}",
                    std::thread::current().id(),
                    round,
                    s,
                );
            }

            // Send successors to workers
            for (i, succ) in successors.into_iter().enumerate() {
                worker.senders[i].send(succ).unwrap();
            }

            // Receive and insert successors
            for _ in 0..worker.shared.num_workers {
                let succ = worker.receiver.recv().unwrap();
                for (prev, state) in succ {
                    if states.insert(Some(prev), state) {
                        next.push(state);
                    }
                }
            }

            round += 1;
        }
    }
}

fn build_workers<const D: usize>() -> (Vec<Worker<D>>, Receiver<States<D>>) {
    let num_workers = num_cpus::get() as u64 - 1;
    let shared = Arc::new(WorkerShared {
        num_workers,
        progress: Mutex::new(0),
        cond_var: Condvar::new(),
        build_hasher: FnvBuildHasher::default(),
        stop_votes: AtomicU64::new(0),
        cleanup: AtomicU64::new(0),
    });
    let (answer_sender, answer_receiver) = channel::<States<D>>();

    let channels: Vec<_> = (0..num_workers).map(|_| channel::<Successors<D>>()).collect();
    let senders: Vec<Vec<_>> = (0..num_workers)
        .map(|_| channels.iter().map(|(s, _)| s.clone()).collect())
        .collect();
    let workers = senders.into_iter()
        .zip(channels.into_iter())
        .map(|(senders, (_, receiver))| Worker {
            shared: Arc::clone(&shared),
            senders,
            receiver,
            answer_sender: answer_sender.clone(),
        })
        .collect();

    (workers, answer_receiver)
}

fn solve<const D: usize, const C: u16>(bananas: u16) -> StateSet<D> {
    let (workers, receiver) = build_workers::<D>();
    let num_workers = workers.len() as u64;

    let held = std::cmp::min(bananas, C);
    let mut initial_state = State {
        inner: StateKey {
            x: 0,
            piles: [0; D],
        },
        held,
    };
    initial_state.inner.piles[0] = bananas - held;

    for (i, worker) in workers.into_iter().enumerate() {
        let mut states = States::<D>::default();
        if i == 0 {
            states.insert(None, initial_state);
        }
        std::thread::spawn(move || {
            worker.run::<C>(states);
        });
    }

    let states = (0..num_workers).map(|_| receiver.recv().unwrap());
    StateSet::new(states)
}

fn main() {
    let start = std::time::Instant::now();
    let solutions = solve::<15, 5>(100);
    let duration = start.elapsed().as_secs_f64();

    println!("visited {} states in {:.2}s", solutions.states[0].len(), duration);
    println!("bananas sold: {}", solutions.bananas_sold);

    if let Some(state) = solutions.solutions.first() {
        println!("{:?}", state);
    } else {
        println!("no solutions found.");
    }
}