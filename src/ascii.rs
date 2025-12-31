//! Visualizes solutions to Cammy the Camel as ASCII animations

use crate::State;
use crate::asciicast::{Animation, Texel};

const WHITE: [u8; 3] = [0xff, 0xff, 0xff];
const BLACK: [u8; 3] = [0, 0, 0];
const GREY: [u8; 3] = [0x7f, 0x7f, 0x7f];
const ORANGE: [u8; 3] = [0xff, 0xa5, 0x00];

// Example frame:
//
//              22
//              C
// ``` ``` ``` ``` ```
//  12   2   0   1   0
struct Frame {
    data: Vec<Texel>,
    width: u32,
    pile_width: u32,
}

impl Frame {
    fn draw(&mut self, x: u32, y: u32, ch: char, fg: [u8; 3], bg: [u8; 3]) {
        let idx = y * self.width + x;
        self.data[idx as usize] = Texel { ch, fg, bg };
    }

    fn draw_num(&mut self, x: u32, y: u32, value: u32, fg: [u8; 3], bg: [u8; 3]) {
        let s = value.to_string();
        for (i, ch) in s.chars().rev().enumerate() {
            // Align right
            self.draw(x - (i as u32) - 1, y, ch, fg, bg);
        }
    }
    
    fn draw_pile(&mut self, index: u32, bananas: u32) {
        let x_0 = index * (self.pile_width + 1);
        let y_0 = 2;
        for i in 0..self.pile_width {
            self.draw(x_0 + i, y_0, '`', WHITE, BLACK);
        }
        let fg = if bananas > 0 { WHITE } else { GREY };
        self.draw_num(x_0 + self.pile_width, y_0 + 1, bananas, fg, BLACK)
    }

    fn draw_cammy(&mut self, index: u32, held: u32) {
        let x_0 = index * (self.pile_width + 1);
        let y_0 = 0;
        self.draw_num(x_0 + self.pile_width, y_0, held, WHITE, BLACK);
        self.draw(x_0 + self.pile_width / 2, y_0 + 1, 'C', ORANGE, BLACK);
    }
}

pub fn render<const D: usize>(path: &[State<D>]) -> Animation {
    let max_num = path.iter()
        .flat_map(|s| std::iter::once(s.held).chain(s.inner.piles.iter().cloned()))
        .max()
        .unwrap();
    let pile_width = max_num.to_string().len() as u32;

    let width = (pile_width + 1) * (D as u32);
    let height = 4;
    let blank = Texel { ch: ' ', fg: WHITE, bg: BLACK };

    let mut anim = Animation {
        data: Vec::new(),
        cols: width,
        rows: height,
        frames: 0,
        fps: 4.0,
    };

    let mut frame = Frame {
        data: vec![blank; (width * height) as usize],
        width,
        pile_width,
    };

    for state in path.iter() {
        // Clear frame
        for texel in frame.data.iter_mut() {
            *texel = blank;
        }

        // Draw piles
        for (i, &num) in state.inner.piles.iter().enumerate() {
            frame.draw_pile(i as u32, num as u32);
        }

        // Draw Cammy
        frame.draw_cammy(state.inner.x as u32, state.held as u32);

        // Append frame to animation
        anim.push_frame(&frame.data[..]);
    }

    anim
}
