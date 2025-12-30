//! Solves the "Cammy the Camel" problem: Cammy lives D miles from the market
//! and has B bananas she wishes to sell. She must eat one banana per mile
//! traveled. She can carry at most C bananas at a time, but she can leave
//! banana piles anywhere she likes on the road and pick them up later. How
//! many bananas can she sell?

#![feature(iter_macro, yield_expr)]

use std::hash::{BuildHasher, Hash};
use std::iter::iter;
use std::sync::{Arc, Condvar, Mutex};
use std::sync::mpsc::{Receiver, Sender, channel};

use dashmap::{DashMap, Entry};
use fnv::FnvBuildHasher;

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

struct WorkerShared<const D: usize> {
    num_workers: u64,
    states: DashMap<StateKey<D>, StateMeta<D>>,
    progress: Mutex<u64>,
    cond_var: Condvar,
    build_hasher: FnvBuildHasher,
}

impl<const D: usize> WorkerShared<D> {
    fn synchronize(&self, level: u64) {
        let mut progress = self.progress.lock().unwrap();
        *progress += 1;
        let target = (level + 1) * self.num_workers;
        if *progress < target {
            while *progress < target {
                progress = self.cond_var.wait(progress).unwrap();
            }
        } else {
            self.cond_var.notify_all();
        }
    }
}

type Work<const D: usize> = Vec<State<D>>;

#[derive(Debug, Default)]
struct Answer<const D: usize> {
    bananas_sold: u16,
    solutions: Vec<State<D>>,
}

impl<const D: usize> Answer<D> {
    fn insert(&mut self, state: &State<D>) {
        if state.inner.x == (D - 1) as u16 {
            if state.held > self.bananas_sold {
                self.bananas_sold = state.held;
                self.solutions.clear();
                self.solutions.push(*state);
            } else if state.held == self.bananas_sold {
                self.solutions.push(*state);
            }
        }
    }

    fn extend(&mut self, other: Self) {
        if other.bananas_sold > self.bananas_sold {
            *self = other;
        } else if other.bananas_sold == self.bananas_sold {
            self.solutions.extend(other.solutions);
        }
    }
}

struct Worker<const D: usize> {
    shared: Arc<WorkerShared<D>>,
    senders: Vec<Sender<Work<D>>>,
    receiver: Receiver<Work<D>>,
    answer: Answer<D>,
    answer_sender: Sender<Answer<D>>,
}

impl<const D: usize> Worker<D> {
    fn insert(&mut self, prev: Option<State<D>>, state: State<D>) -> bool {
        let key = state.inner;
        let meta = StateMeta {
            prev,
            held: state.held,
        };
        match self.shared.states.entry(key) {
            Entry::Occupied(e) => {
                // Because we traverse in BF order, we always visit states with
                // highest held value first
                assert!(e.get().held >= state.held);
                false
            }
            Entry::Vacant(e) => {
                e.insert(meta);
                self.answer.insert(&state);
                true
            }
        }
    }

    fn run<const C: u16>(self, depth: u64, mut work: Vec<Work<D>>) {
        let mut worker = self;
        for level in 0..depth {
            // Do not send any work until all work from previous
            // level has been received
            worker.shared.synchronize(level);

            // Compute successors, sharded on hash
            let mut new_work: Vec<Work<D>> = vec![vec![]; worker.shared.num_workers as usize];
            for state in std::mem::take(&mut work).into_iter().flat_map(|v| v.into_iter()) {
                for successor in state.successors::<C>() {
                    if worker.insert(Some(state), successor) {
                        // We only use hashing to distribute work; the hash
                        // here is not expected to match the hash used by the
                        // state map
                        let hash = worker.shared.build_hasher.hash_one(&successor);
                        let shard = (hash % worker.shared.num_workers) as usize;
                        new_work[shard].push(successor);
                    }
                }
            }

            // Distribute work
            for (i, w) in new_work.into_iter().enumerate() {
                worker.senders[i].send(w).unwrap();
            }

            // Receive work
            for _ in 0..worker.shared.num_workers {
                work.push(worker.receiver.recv().unwrap());
            }
        }
        
        assert_eq!(work.iter().map(|v| v.len()).sum::<usize>(), 0);
        worker.answer_sender.send(worker.answer).unwrap();
    }
}

fn build_workers<const D: usize>() -> (Vec<Worker<D>>, Receiver<Answer<D>>) {
    let num_workers = num_cpus::get() as u64 - 1;
    let shared = Arc::new(WorkerShared {
        num_workers,
        states: Default::default(),
        progress: Mutex::new(0),
        cond_var: Condvar::new(),
        build_hasher: FnvBuildHasher::default(),
    });
    let (answer_sender, answer_receiver) = channel::<Answer<D>>();

    let channels: Vec<_> = (0..num_workers).map(|_| channel::<Work<D>>()).collect();
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
            answer: Default::default(),
        })
        .collect();

    (workers, answer_receiver)
}

fn solve<const D: usize, const C: u16>(bananas: u16) -> (DashMap<StateKey<D>, StateMeta<D>>, Answer<D>) {
    let depth = bananas + 1;

    let (workers, receiver) = build_workers::<D>();
    let num_workers = workers.len() as u64;
    let shared = Arc::clone(&workers[0].shared);

    let held = std::cmp::min(bananas, C);
    let mut initial_state = State {
        inner: StateKey {
            x: 0,
            piles: [0; D],
        },
        held,
    };
    initial_state.inner.piles[0] = bananas - held;
    workers[0].shared.states.insert(initial_state.inner, StateMeta { prev: None, held });

    let mut threads = vec![];
    for (i, worker) in workers.into_iter().enumerate() {
        let mut work = vec![];
        if i == 0 {
            work.push(vec![initial_state]);
        }
        threads.push(std::thread::spawn(move || {
            worker.run::<C>(depth as u64, work);
        }));
    }

    for handle in threads {
        handle.join().unwrap();
    }

    let mut answer = Answer::default();
    for a in (0..num_workers).map(|_| receiver.recv().unwrap()) {
        answer.extend(a);
    }

    let Ok(shared) = Arc::try_unwrap(shared) else {
        panic!("Arc::unwrap failed");
    };

    (shared.states, answer)
}

fn main() {
    let start = std::time::Instant::now();
    let (states, solutions) = solve::<10, 5>(170);
    let duration = start.elapsed().as_secs_f64();

    let num_states = states.len();
    println!("visited {} states in {:.2}s", num_states, duration);
    println!("bananas sold: {}", solutions.bananas_sold);

    if let Some(state) = solutions.solutions.first() {
        println!("{:?}", state);
    } else {
        let farthest = states.iter().max_by_key(|r| r.key().x).unwrap();
        println!("no solutions found. farthest reached: {}", farthest.key().x);
        println!("{:?}, {:?}", farthest.key(), farthest.value());
    }
}
