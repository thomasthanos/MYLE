// ! Turns the raw bytes of a console tool into finished lines. ! ! Three things make this harder than `String::from_utf8_lossy`: ! - `sfc.exe` writes UTF-16LE **without a BOM**, the rest write single-byte ! text: UTF-8 (winget), or the console's OEM code page (`ipconfig`, ! `chkdsk`, DISM), which on a Greek Windows is CP737, not UTF-8; ! - a read can end in the middle of a line, or of a UTF-16 code unit; ! - progress output repaints the same line, so the bytes have to be replayed ! the way a terminal would draw them rather than split on every `\r`. ! ! The last one is the subtle one. `\r` does not end a line: it moves the ! cursor back to the start, and whatever is typed next overwrites what is ! there. Only `\n` commits a line. So `"50%\r100%\r\n"` is the single line ! `"100%"`, not three lines, and not an empty one.
/// Enough bytes to tell UTF-16LE from single-byte text without a BOM.
const SNIFF_MIN: usize = 16;
const SNIFF_MAX: usize = 512;
/// A line this long without a newline is committed anyway, so a tool that
/// never writes one cannot grow the buffer without bound.
const MAX_LINE: usize = 64 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16Le,
    /// The OEM code page. Chosen once the text turns out not to be UTF-8.
    Oem,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    /// Replaces the line emitted before it, which was still being drawn.
    pub replace: bool,
}
#[derive(Default)]
pub struct LineSplitter {
    encoding: Option<Encoding>,
    /// Bytes that do not yet form whole characters.
    bytes: Vec<u8>,
    /// The line the cursor is on.
    current: String,
    /// A carriage return is pending: the next character starts from column 0.
    overwrite: bool,
    /// The last line handed out was this same, unfinished line.
    provisional: bool,
}
impl LineSplitter {
    /// Every line that is finished, plus the one still being drawn.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Line> {
        self.bytes.extend_from_slice(chunk);
        if self.encoding.is_none() {
            self.encoding = sniff(&self.bytes);
            // Not enough bytes to be sure yet; hold everything back.
            if self.encoding.is_none() {
                return Vec::new();
            }
        }
        let text = self.take_decodable();
        let mut lines = Vec::new();
        for c in text.chars() {
            match c {
                '\r' => self.overwrite = true,
                '\n' => lines.push(self.commit()),
                '\u{8}' => {
                    self.current.pop();
                }
                // Control and padding characters a terminal would not show.
                '\u{0}' | '\u{feff}' => {}
                _ => {
                    if self.overwrite {
                        self.current.clear();
                        self.overwrite = false;
                    }
                    self.current.push(c);
                    if self.current.len() >= MAX_LINE {
                        lines.push(self.commit());
                    }
                }
            }
        }
        // Show the line in progress, so a repainting percentage is live.
        if !self.current.is_empty() {
            lines.push(self.draw());
        }
        lines
    }
    /// The last line, once the process has exited. Idempotent.
    pub fn finish(&mut self) -> Option<Line> {
        let encoding = self.encoding.or_else(|| sniff_final(&self.bytes))?;
        self.encoding = Some(encoding);
        let mut text = self.take_decodable();
        // A "character" still cut off once the output has ended was never UTF-8: it is the last letter of OEM text.
        if !self.bytes.is_empty() && self.encoding == Some(Encoding::Utf8) {
            self.encoding = Some(Encoding::Oem);
            text += &self.take_decodable();
        }
        for c in text.chars() {
            if c == '\u{8}' {
                self.current.pop();
                continue;
            }
            if !matches!(c, '\r' | '\n' | '\u{0}' | '\u{feff}') {
                if self.overwrite {
                    self.current.clear();
                    self.overwrite = false;
                }
                self.current.push(c);
            }
        }
        (!self.current.is_empty()).then(|| self.commit())
    }
    /// Ends the current line and starts a new one.
    fn commit(&mut self) -> Line {
        let line = Line {
            text: std::mem::take(&mut self.current).trim_end().to_string(),
            replace: self.provisional,
        };
        self.overwrite = false;
        self.provisional = false;
        line
    }
    /// Hands out the unfinished line; the next one sent replaces it.
    fn draw(&mut self) -> Line {
        let line = Line {
            text: self.current.trim_end().to_string(),
            replace: self.provisional,
        };
        self.provisional = true;
        line
    }
    /// Decodes as much of the buffer as forms whole characters, keeping any
    /// partial one for the next read.
    fn take_decodable(&mut self) -> String {
        match self.encoding {
            Some(Encoding::Utf16Le) => {
                let mut take = self.bytes.len() - self.bytes.len() % 2;
                // Keep a leading surrogate with the trailing one it needs.
                if take >= 2 {
                    let last = u16::from_le_bytes([self.bytes[take - 2], self.bytes[take - 1]]);
                    if (0xD800..0xDC00).contains(&last) {
                        take -= 2;
                    }
                }
                let whole: Vec<u8> = self.bytes.drain(..take).collect();
                decode(&whole, Encoding::Utf16Le)
            }
            Some(Encoding::Oem) => {
                let whole = std::mem::take(&mut self.bytes);
                decode(&whole, Encoding::Oem)
            }
            _ => match std::str::from_utf8(&self.bytes) {
                Ok(_) => {
                    let whole = std::mem::take(&mut self.bytes);
                    decode(&whole, Encoding::Utf8)
                }
                // A byte sequence UTF-8 does not allow: this is the OEM code page after all. Its ASCII half is the same, so what was already shown stays right.
                Err(e) if e.error_len().is_some() => {
                    self.encoding = Some(Encoding::Oem);
                    let whole = std::mem::take(&mut self.bytes);
                    decode(&whole, Encoding::Oem)
                }
                // A character cut off by the read waits for the rest.
                Err(e) => {
                    let whole: Vec<u8> = self.bytes.drain(..e.valid_up_to()).collect();
                    decode(&whole, Encoding::Utf8)
                }
            },
        }
    }
}
/// The encoding of a stream, once there is enough of it to tell.
pub fn sniff(head: &[u8]) -> Option<Encoding> {
    if head.starts_with(&[0xFF, 0xFE]) {
        return Some(Encoding::Utf16Le);
    }
    if head.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Some(Encoding::Utf8);
    }
    (head.len() >= SNIFF_MIN).then(|| sniff_final(head).unwrap_or(Encoding::Utf8))
}
/// Same decision, forced: used once the process has exited and no more bytes
/// are coming, so a two-line run still decodes.
fn sniff_final(head: &[u8]) -> Option<Encoding> {
    if head.is_empty() {
        return None;
    }
    if head.starts_with(&[0xFF, 0xFE]) {
        return Some(Encoding::Utf16Le);
    }
    let sample = &head[..head.len().min(SNIFF_MAX)];
    let odd = sample.len() / 2;
    if odd == 0 {
        return Some(Encoding::Utf8);
    }
    // Western text in UTF-16LE is every other byte zero; UTF-8 has almost none.
    let zeros = sample
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|b| **b == 0)
        .count();
    if zeros * 10 >= odd * 7 {
        Some(Encoding::Utf16Le)
    } else {
        Some(Encoding::Utf8)
    }
}
pub fn decode(bytes: &[u8], encoding: Encoding) -> String {
    match encoding {
        Encoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        Encoding::Oem => decode_oem(bytes),
        Encoding::Utf16Le => {
            let (pairs, _odd) = bytes.as_chunks::<2>();
            let units: Vec<u16> = pairs.iter().copied().map(u16::from_le_bytes).collect();
            String::from_utf16_lossy(&units)
        }
    }
}
/// Text in the console's OEM code page, the way `cmd.exe` would show it.
fn decode_oem(bytes: &[u8]) -> String {
    use windows_sys::Win32::Globalization::{CP_OEMCP, MultiByteToWideChar};
    let Ok(length) = i32::try_from(bytes.len()) else {
        return String::from_utf8_lossy(bytes).into_owned();
    };
    if length == 0 {
        return String::new();
    }
    // Single-byte code pages give one UTF-16 unit per byte.
    let mut wide = vec![0u16; bytes.len()];
    // SAFETY: both buffers are valid for the lengths passed with them.
    let written = unsafe {
        MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr(),
            length,
            wide.as_mut_ptr(),
            length,
        )
    };
    match usize::try_from(written) {
        Ok(written) if written > 0 => String::from_utf16_lossy(&wide[..written]),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }
    /// What the page ends up showing: `replace` overwrites the line before it.
    fn drawn(lines: &[Line]) -> Vec<String> {
        let mut shown: Vec<String> = Vec::new();
        for line in lines {
            if line.replace && !shown.is_empty() {
                *shown.last_mut().unwrap() = line.text.clone();
            } else {
                shown.push(line.text.clone());
            }
        }
        shown
    }
    #[test]
    fn sniffs_the_encodings_we_actually_meet() {
        assert_eq!(sniff(&[0xFF, 0xFE, b'a', 0]), Some(Encoding::Utf16Le));
        assert_eq!(sniff(&[0xEF, 0xBB, 0xBF, b'a']), Some(Encoding::Utf8));
        // sfc.exe: UTF-16LE with no BOM.
        assert_eq!(
            sniff(&utf16("Beginning system scan.")),
            Some(Encoding::Utf16Le)
        );
        assert_eq!(sniff(b"Deployment Image Servicing"), Some(Encoding::Utf8));
        assert_eq!(sniff(b""), None);
        assert_eq!(sniff(b"short"), None, "waits for enough bytes to be sure");
    }
    #[test]
    fn text_that_is_not_utf8_is_read_in_the_oem_code_page() {
        let mut splitter = LineSplitter::default();
        // 0x82 cannot appear on its own in UTF-8; in an OEM code page it is a letter ("é" in CP437, "Γ" in CP737), whichever this PC uses.
        let mut lines = splitter.feed(b"Windows IP Configuration\r\nCaf\x82 ok\r\nEnd \xE0");
        lines.extend(splitter.finish());
        let shown = drawn(&lines);
        assert_eq!(shown[0], "Windows IP Configuration");
        assert!(shown[1].starts_with("Caf") && shown[1].ends_with(" ok"));
        assert_eq!(shown[1].chars().count(), 7);
        assert!(shown[2].starts_with("End ") && shown[2].chars().count() == 5);
        assert!(!shown.concat().contains('\u{fffd}'));
    }
    #[test]
    fn decodes_utf16_and_survives_a_lone_surrogate() {
        assert_eq!(decode(&utf16("héllo"), Encoding::Utf16Le), "héllo");
        let lone = [0x00, 0xD8, b'a', 0x00];
        assert_eq!(decode(&lone, Encoding::Utf16Le), "\u{fffd}a");
    }
    /// The bug this rewrite fixes: `\r\n` after a repainted line used to blank
    /// it, so a finished scan showed empty rows instead of its own output.
    #[test]
    fn a_real_sfc_scan_reads_the_way_it_looks_in_a_console() {
        let mut splitter = LineSplitter::default();
        let mut lines = splitter.feed(&utf16(
            "\r\nBeginning system scan.  This process will take some time.\r\n\r\n\
             Beginning verification phase of system scan.\r\n\
             Verification 20% complete.\rVerification 60% complete.\r\
             Verification 100% complete.\r\n\
             Windows Resource Protection did not find any integrity violations.\r\n",
        ));
        lines.extend(splitter.finish());
        assert_eq!(
            drawn(&lines),
            [
                "",
                "Beginning system scan.  This process will take some time.",
                "",
                "Beginning verification phase of system scan.",
                "Verification 100% complete.",
                "Windows Resource Protection did not find any integrity violations.",
            ]
        );
    }
    #[test]
    fn a_repaint_arriving_on_its_own_is_shown_at_once_and_then_replaced() {
        let mut splitter = LineSplitter::default();
        let first = splitter.feed(b"Beginning verification phase.\nVerification 20% complete.\r");
        assert_eq!(
            drawn(&first),
            [
                "Beginning verification phase.",
                "Verification 20% complete."
            ]
        );
        let second = splitter.feed(b"Verification 60% complete.\r");
        // It repaints the same line rather than adding one.
        assert!(second[0].replace);
        assert_eq!(second[0].text, "Verification 60% complete.");
    }
    #[test]
    fn a_line_split_across_two_reads_arrives_whole() {
        let mut splitter = LineSplitter::default();
        let mut lines = splitter.feed(b"Deployment Image Ser");
        lines.extend(splitter.feed(b"vicing\nnext line\n"));
        assert_eq!(drawn(&lines), ["Deployment Image Servicing", "next line"]);
    }
    #[test]
    fn utf16_split_on_an_odd_byte_still_decodes() {
        let bytes = utf16("Beginning system scan.\r\ndone\r\n");
        let mut splitter = LineSplitter::default();
        // Cut in the middle of a code unit.
        let mut lines = splitter.feed(&bytes[..21]);
        lines.extend(splitter.feed(&bytes[21..]));
        assert_eq!(drawn(&lines), ["Beginning system scan.", "done"]);
    }
    #[test]
    fn crlf_is_one_terminator() {
        let mut splitter = LineSplitter::default();
        let lines = splitter.feed(b"first line here\r\nsecond line here\r\n");
        assert_eq!(drawn(&lines), ["first line here", "second line here"]);
    }
    #[test]
    fn backspaces_repaint_progress_instead_of_reaching_the_page() {
        let mut splitter = LineSplitter::default();
        let mut lines = splitter.feed(b"Verification 20%\x08\x08\x0860% complete.\r\n");
        lines.extend(splitter.finish());
        assert_eq!(drawn(&lines), ["Verification 60% complete."]);
    }
    #[test]
    fn blank_lines_are_kept_but_a_repaint_never_blanks_a_real_one() {
        let mut splitter = LineSplitter::default();
        let lines = splitter.feed(b"working on it\r\n\r\nstill here\r\n");
        assert_eq!(drawn(&lines), ["working on it", "", "still here"]);
    }
    #[test]
    fn finish_emits_the_tail_once() {
        let mut splitter = LineSplitter::default();
        splitter.feed(b"a full line here\nand a partial one");
        assert_eq!(splitter.finish().unwrap().text, "and a partial one");
        assert_eq!(splitter.finish(), None);
    }
    #[test]
    fn finish_decodes_output_too_short_to_sniff() {
        let mut splitter = LineSplitter::default();
        assert!(splitter.feed(b"Ok").is_empty());
        assert_eq!(splitter.finish().unwrap().text, "Ok");
    }
    #[test]
    fn a_tool_that_never_writes_a_newline_cannot_grow_the_buffer() {
        let mut splitter = LineSplitter::default();
        let mut committed = 0;
        for _ in 0..16 {
            committed += splitter
                .feed(&vec![b'x'; 64 * 1024])
                .iter()
                .filter(|l| !l.replace)
                .count();
            assert!(splitter.current.len() < MAX_LINE);
            assert!(splitter.bytes.len() < 4);
        }
        assert!(
            committed >= 15,
            "the overlong line is committed, not buffered"
        );
    }
    #[test]
    fn nuls_and_boms_never_reach_the_page() {
        let mut splitter = LineSplitter::default();
        let mut lines = splitter.feed(&utf16("\u{feff}hello there\u{0}\u{0}  \n"));
        lines.extend(splitter.finish());
        assert_eq!(drawn(&lines), ["hello there"]);
    }
}
