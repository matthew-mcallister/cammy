//! Solves the "Cammy the Camel" problem: Cammy lives D miles from the market
//! and has B bananas she wishes to sell. She must eat one banana per mile
//! traveled. She can carry at most C bananas at a time, but she can leave
//! banana piles anywhere she likes on the road and pick them up later. How
//! many bananas can she sell?

#![feature(iter_macro, yield_expr)]

use std::iter::iter;

use fnv::FnvHashMap;

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
struct State<const D: usize> {
    x: u16,
    held: u16,
    piles: [u16; D],
}

impl<const D: usize> State<D> {
    fn moves(&self) -> impl Iterator<Item = State<D>> + '_ {
        let state = self;
        let it = iter!(move || {
            if state.held == 0 { return; }
            if state.x + 1 < D as u16 {
                State {
                    x: state.x + 1,
                    held: state.held - 1,
                    ..*state
                }.yield
            }
            if state.x > 0 {
                State {
                    x: state.x - 1,
                    held: state.held - 1,
                    ..*state
                }.yield
            }
        });
        it()
    }

    fn pickup<const C: u16>(&mut self) {
        let state = self;
        let pile = state.x as usize;
        let pickup = std::cmp::min(C - state.held, state.piles[pile]);
        state.held += pickup;
        state.piles[pile] -= pickup;
    }

    fn successors<const C: u16>(&self) -> impl Iterator<Item = State<D>> + '_ {
        let state = self;
        let it = iter!(move || {
            for mut state in state.moves() {
                state.pickup::<C>();
                for amount in 0..=state.held {
                    let mut state = state;
                    state.held -= amount;
                    state.piles[state.x as usize] += amount;
                    state.yield;
                }
            }
        });
        it()
    }
}

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
struct Node<const D: usize> {
    prev: Option<State<D>>,
    visited: bool,
}

#[derive(Debug)]
struct Solver<const D: usize> {
    states: FnvHashMap<State<D>, Node<D>>,
}

impl<const D: usize> Solver<D> {
    /// Returns an iterator over states that attain maximal distance from
    /// start, with maximal number of bananas held at that position.
    fn solutions(&self) -> impl Iterator<Item = (&'_ State<D>, &'_ Node<D>)> + '_ {
        let max_x = self.states.keys().map(|s| s.x).max().unwrap_or(0);
        let max_held = self.states.keys()
            .filter(|s| s.x == max_x)
            .map(|s| s.held)
            .max()
            .unwrap_or(0);
        self.states.iter()
            .filter(move |(s, _)| s.x == max_x && s.held == max_held)
    }
}

fn solve<const D: usize, const C: u16>(bananas: u16) -> Solver<D> {
    let start = std::time::Instant::now();

    let mut solver = Solver {
        states: Default::default(),
    };
    let mut initial = State {
        x: 0,
        held: 0,
        piles: [0; D],
    };
    initial.piles[0] = bananas;
    initial.pickup::<C>();
    solver.states.insert(initial, Node { prev: None, visited: false });

    let mut work = vec![initial];
    while let Some(state) = work.pop() {
        let node = solver.states.get_mut(&state).unwrap();
        if node.visited {
            continue;
        }
        node.visited = true;
        
        for succ in state.successors::<C>() {
            solver.states.entry(succ).or_insert(Node { prev: Some(state), visited: false });
            work.push(succ);
        }
    }

    let elapsed = start.elapsed();

    println!("done solving. visited {} states in {:.2}s", solver.states.len(), elapsed.as_secs_f64());

    solver
}

fn main() {
    let solver = solve::<15, 5>(120);
    let (state, _node) = solver.solutions().next().unwrap();
    println!("{:?}", state);
}