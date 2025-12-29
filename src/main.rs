//! Solves the "Cammy the Camel" problem: Cammy lives D miles from the market
//! and has B bananas she wishes to sell. She must eat one banana per mile
//! traveled. She can carry at most C bananas at a time, but she can leave
//! banana piles anywhere she likes on the road and pick them up later. How
//! many bananas can she sell?

#![feature(iter_macro, yield_expr)]

use std::collections::VecDeque;
use std::collections::hash_map::Entry;
use std::iter::iter;

use fnv::FnvHashMap;

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

impl<const D: usize> State<D> {
    fn moves(&self) -> impl Iterator<Item = State<D>> + '_ {
        let state = self;
        let it = iter!(move || {
            if state.held == 0 { return; }
            if state.inner.x + 1 < D as u16 {
                State {
                    inner: StateKey {
                        x: state.inner.x + 1,
                        piles: state.inner.piles,
                    },
                    held: state.held - 1,
                    ..*state
                }.yield
            }
            if state.inner.x > 0 {
                State {
                    inner: StateKey {
                        x: state.inner.x - 1,
                        piles: state.inner.piles,
                    },
                    held: state.held - 1,
                    ..*state
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

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
struct StateMeta<const D: usize> {
    prev: Option<StateKey<D>>,
    held: u16,
}

#[derive(Debug, Default)]
struct Solver<const D: usize> {
    states: FnvHashMap<StateKey<D>, StateMeta<D>>,
    bananas_sold: u16,
    solutions: Vec<State<D>>,
}

impl<const D: usize> Solver<D> {
    fn insert(&mut self, prev: Option<StateKey<D>>, state: State<D>) -> bool {
        let meta = StateMeta {
            prev,
            held: state.held,
        };
        match self.states.entry(state.inner) {
            Entry::Occupied(e) => {
                // We never overwrite an entry because we visit in order of
                // depth from the root
                assert!(e.get().held >= meta.held);
                false
            }
            Entry::Vacant(e) => {
                e.insert(meta);
                if state.inner.x == (D - 1) as u16 {
                    if state.held > self.bananas_sold {
                        self.bananas_sold = state.held;
                        self.solutions = vec![state];
                    } else if state.held == self.bananas_sold {
                        self.solutions.push(state);
                    }
                }
                true
            }
        }
    }
}

fn solve<const D: usize, const C: u16>(bananas: u16) -> Solver<D> {
    let start = std::time::Instant::now();

    let mut solver = Solver::default();
    let held = std::cmp::min(bananas, C);
    let mut initial = State {
        inner: StateKey {
            x: 0,
            piles: [0; D],
        },
        held,
    };
    initial.inner.piles[0] = bananas - held;
    solver.insert(None, initial);

    let mut work = VecDeque::new();
    work.push_back(initial);

    while let Some(state) = work.pop_front() {
        for succ in state.successors::<C>() {
            if solver.insert(Some(state.inner), succ) {
                work.push_back(succ);
            }
        }
    }

    let elapsed = start.elapsed();

    println!("visited {} states in {:.2}s", solver.states.len(), elapsed.as_secs_f64());

    solver
}

fn main() {
    let solver = solve::<15, 5>(100);
    if let Some(s) = solver.solutions.first() {
        println!("{:?}", s);
    } else {
        println!("no solutions.");
    }
}