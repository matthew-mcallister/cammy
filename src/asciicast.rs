//! Output ASCII animation as asciicast v2 for encodeing with asciinema.

use std::fmt::Write as FmtWrite;
use std::io;
use std::io::Write;

/// Escapes a string to be valid JSON
fn write_escaped<W: Write>(w: &mut W, s: &str) -> io::Result<()> {
    for c in s.chars() {
        match c {
            '"' => write!(w, "\\\"")?,
            '\\' => write!(w, r"\\")?,
            '\n' => write!(w, r"\n")?,
            '\r' => write!(w, r"\r")?,
            '\t' => write!(w, r"\t")?,
            c if c as u32 <= 0x1f => write!(w, r"\u00{:0x}", c as u32)?,
            c => write!(w, "{}", c)?,
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Texel {
    pub ch: char,
    pub fg: [u8; 3],
    pub bg: [u8; 3],
}

/// ASCII animation data. Texels stored in row-major order.
#[derive(Debug, Clone)]
pub struct Animation {
    pub data: Vec<Texel>,
    pub cols: u32,
    pub rows: u32,
    pub frames: u32,
    pub fps: f32,
}

impl Animation {
    pub fn encode_to_asciicast_v2<W: Write>(&self, w: &mut W) -> io::Result<()> {
        let expected_size = self.cols * self.rows * self.frames;
        assert_eq!(self.data.len(), expected_size as usize);

        // Header
        writeln!(w, "{{\"version\": 2, \"width\": {}, \"height\": {}}}", self.cols, self.rows)?;

        let mut frame = String::new();
        let mut fg;
        let mut bg;
        for f in 0..self.frames {
            frame.clear();

            // Reset cursor to (0, 0). The screen isn't cleared, which prevents
            // flickering
            frame.push_str("\x1b[0m\x1b[H");
            fg = None;
            bg = None;

            for y in 0..self.rows {
                for x in 0..self.cols {
                    let idx = f * self.cols * self.rows + y * self.cols + x;
                    let texel = &self.data[idx as usize];

                    if fg != Some(texel.fg) {
                        write!(
                            &mut frame,
                            "\x1b[38;2;{};{};{}m",
                            texel.fg[0], texel.fg[1], texel.fg[2]
                        ).unwrap();
                        fg = Some(texel.fg);
                    }

                    if bg != Some(texel.bg) {
                        write!(
                            &mut frame,
                            "\x1b[48;2;{};{};{}m",
                            texel.bg[0], texel.bg[1], texel.bg[2]
                        ).unwrap();
                        bg = Some(texel.bg);
                    }

                    frame.push(texel.ch);
                }
                
                // Reset colors at end of line to prevent background bleeding
                frame.push_str("\x1b[0m");
                if y < self.rows - 1 {
                    frame.push_str("\r\n");
                }
                fg = None;
                bg = None;
            }

            let time = f as f32 / self.fps;
            write!(w, "[{}, \"o\", \"", time)?;
            write_escaped(w, &frame)?;
            writeln!(w, "\"]")?;
        }

        Ok(())
    }

    pub fn push_frame(&mut self, frame: &[Texel]) {
        assert_eq!(frame.len(), (self.cols * self.rows) as usize);
        self.data.extend_from_slice(frame);
        self.frames += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render() {
        let mut anim = Animation {
            data: vec![],
            cols: 2,
            rows: 2,
            frames: 0,
            fps: 1.0,
        };

        // Create a 2x2 animation, 2 frames long
        let red = [255, 0, 0];
        let blue = [0, 0, 255];
        let black = [0, 0, 0];
        
        // Frame 1: Red 'A's on Black
        anim.push_frame(&[
            Texel { ch: 'A', fg: red, bg: black }, Texel { ch: 'A', fg: red, bg: black },
            Texel { ch: 'A', fg: red, bg: black }, Texel { ch: 'A', fg: red, bg: black },
        ]);
        
        // Frame 2: Blue 'B's on Black
        anim.push_frame(&[
            Texel { ch: 'B', fg: blue, bg: black }, Texel { ch: 'B', fg: blue, bg: black },
            Texel { ch: 'B', fg: blue, bg: black }, Texel { ch: 'B', fg: blue, bg: black },
        ]);

        let mut output = Vec::new();
        anim.encode_to_asciicast_v2(&mut output).expect("Failed to encode");
        
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output, concat!(
            "{\"version\": 2, \"cols\": 2, \"rows\": 2}\n",
            "[0, \"o\", \"\\u001b[0m\\u001b[H\\u001b[38;2;255;0;0m\\u001b[48;2;0;0;0mAA\\u001b[0m\\r\\n\\u001b[38;2;255;0;0m\\u001b[48;2;0;0;0mAA\\u001b[0m\\r\\n\"]\n",
            "[1, \"o\", \"\\u001b[0m\\u001b[H\\u001b[38;2;0;0;255m\\u001b[48;2;0;0;0mBB\\u001b[0m\\r\\n\\u001b[38;2;0;0;255m\\u001b[48;2;0;0;0mBB\\u001b[0m\\r\\n\"]\n",
        ));
    }
}