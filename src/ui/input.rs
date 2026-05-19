use compact_str::CompactString;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::picker::FilePicker;

const KILL_RING_MAX: usize = 10;

// `cursor` is a byte-offset into `buffer` (UTF-8). The helpers below move the
// cursor by one character boundary so we never land in the middle of a
// multibyte sequence — that would panic on the next insert/remove in
// `CompactString`/`String`.
enum KillDir {
    Prepend,
    Append,
}

#[derive(Default)]
struct YankState {
    index: usize,
    cursor: usize,
    len: usize,
}

fn prev_char_boundary(s: &str, idx: usize) -> usize {
    let mut i = idx.saturating_sub(1);
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn next_char_boundary(s: &str, idx: usize) -> usize {
    let len = s.len();
    let mut i = (idx + 1).min(len);
    while i < len && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn is_whitespace(ch: char) -> bool {
    ch.is_whitespace()
}

fn prev_word_boundary(s: &str, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }
    let mut idx = prev_char_boundary(s, cursor);
    // skip trailing whitespace
    while idx > 0 {
        let ch = s[..idx].chars().next_back().unwrap_or(' ');
        if !is_whitespace(ch) {
            break;
        }
        idx = prev_char_boundary(s, idx);
    }
    // determine character class at position
    if idx == 0 {
        return 0;
    }
    let ch = s[..idx].chars().next_back().unwrap_or(' ');
    let is_word = is_word_char(ch);
    // skip backward through same class
    while idx > 0 {
        let ch = s[..idx].chars().next_back().unwrap_or(' ');
        let current_is_word = is_word_char(ch);
        if current_is_word != is_word || is_whitespace(ch) {
            break;
        }
        let prev = prev_char_boundary(s, idx);
        if prev == idx {
            break;
        }
        idx = prev;
    }
    idx
}

fn next_word_boundary(s: &str, cursor: usize) -> usize {
    let len = s.len();
    if cursor >= len {
        return len;
    }
    let ch = s[cursor..].chars().next().unwrap_or(' ');
    let is_word = is_word_char(ch);
    let is_ws = is_whitespace(ch);
    let mut idx = cursor;
    // skip current class (word, punct, or whitespace)
    while idx < len {
        let ch = s[idx..].chars().next().unwrap_or(' ');
        let current_is_word = is_word_char(ch);
        let current_is_ws = is_whitespace(ch);
        if is_ws {
            if !current_is_ws {
                break;
            }
        } else if current_is_word != is_word {
            break;
        }
        idx = next_char_boundary(s, idx);
    }
    // skip whitespace and punctuation between words
    while idx < len {
        let ch = s[idx..].chars().next().unwrap_or(' ');
        if is_word_char(ch) {
            break;
        }
        idx = next_char_boundary(s, idx);
    }
    idx
}

fn cursor_line_start(s: &str, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }
    let haystack = &s[..cursor];
    match haystack.rfind('\n') {
        Some(pos) => pos + 1,
        None => 0,
    }
}

fn prev_line_start(s: &str, cursor: usize) -> Option<usize> {
    let line_start = cursor_line_start(s, cursor);
    if line_start == 0 {
        return None;
    }
    Some(cursor_line_start(s, line_start.saturating_sub(1)))
}

fn next_line_start(s: &str, cursor: usize) -> Option<usize> {
    let after = &s[cursor..];
    after.find('\n').map(|p| cursor + p + 1)
}

pub struct InputEditor {
    pub buffer: CompactString,
    pub cursor: usize,
    history: Vec<CompactString>,
    history_pos: Option<usize>,
    pub picker: Option<FilePicker>,
    monochrome: bool,
    kill_ring: Vec<CompactString>,
    last_action_was_kill: bool,
    yank_state: Option<YankState>,
}

impl InputEditor {
    pub fn new() -> Self {
        InputEditor {
            buffer: CompactString::new(""),
            cursor: 0,
            history: Vec::new(),
            history_pos: None,
            picker: None,
            monochrome: false,
            kill_ring: Vec::new(),
            last_action_was_kill: false,
            yank_state: None,
        }
    }

    pub fn set_monochrome(&mut self, monochrome: bool) {
        self.monochrome = monochrome;
        if let Some(picker) = self.picker.as_mut() {
            picker.set_monochrome(monochrome);
        }
    }

    pub fn start_picker(&mut self) {
        let picker = self.picker.get_or_insert_with(FilePicker::new);
        picker.set_monochrome(self.monochrome);
        picker.activate();
    }

    fn reset_kill_accumulation(&mut self) {
        self.last_action_was_kill = false;
        self.yank_state = None;
    }

    fn push_kill(&mut self, text: CompactString, direction: KillDir) {
        if text.is_empty() {
            return;
        }
        if self.last_action_was_kill && !self.kill_ring.is_empty() {
            let entry = &mut self.kill_ring[0];
            match direction {
                KillDir::Prepend => {
                    let mut new = text;
                    new.push_str(entry);
                    *entry = new;
                }
                KillDir::Append => {
                    entry.push_str(&text);
                }
            }
        } else {
            self.kill_ring.insert(0, text);
            if self.kill_ring.len() > KILL_RING_MAX {
                self.kill_ring.pop();
            }
        }
        self.last_action_was_kill = true;
    }

    pub fn handle_picker_key(&mut self, key: KeyEvent) -> bool {
        let picker = match self.picker.as_mut() {
            Some(p) if p.active => p,
            _ => return false,
        };

        match key.code {
            KeyCode::Char(c)
                if c == '\x08' || (c == 'h' && key.modifiers.contains(KeyModifiers::CONTROL)) =>
            {
                if picker.cursor > 0 {
                    picker.backspace();
                    self.cursor = prev_char_boundary(&self.buffer, self.cursor);
                    self.buffer.remove(self.cursor);
                } else {
                    let at_pos = self.buffer.rfind('@');
                    if let Some(at) = at_pos {
                        let before: String = self.buffer.chars().take(at).collect();
                        let after: String = self.buffer.chars().skip(at + 1).collect();
                        self.buffer = format!("{}{}", before, after).into();
                        self.cursor = at;
                    }
                    picker.deactivate();
                }
                true
            }
            KeyCode::Char(c) => {
                picker.char_input(c);
                self.buffer.insert(self.cursor, c);
                self.cursor += c.len_utf8();
                true
            }
            KeyCode::Backspace => {
                if picker.cursor > 0 {
                    picker.backspace();
                    self.cursor = prev_char_boundary(&self.buffer, self.cursor);
                    self.buffer.remove(self.cursor);
                    true
                } else {
                    let at_pos = self.buffer.rfind('@');
                    if let Some(at) = at_pos {
                        let before: String = self.buffer.chars().take(at).collect();
                        let after: String = self.buffer.chars().skip(at + 1).collect();
                        self.buffer = format!("{}{}", before, after).into();
                        self.cursor = at;
                    }
                    picker.deactivate();
                    true
                }
            }
            KeyCode::Tab => {
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::SHIFT)
                {
                    picker.select_prev();
                } else {
                    picker.select_next();
                }
                true
            }
            KeyCode::Up => {
                picker.select_prev();
                true
            }
            KeyCode::Down => {
                picker.select_next();
                true
            }
            KeyCode::Enter => {
                if let Some(path) = picker.selected_path() {
                    let path_str = path.to_string_lossy().to_string();
                    let at_pos = self.buffer.rfind('@');
                    if let Some(at) = at_pos {
                        let before: String = self.buffer.chars().take(at).collect();
                        let after_offset = at + 1 + picker.query.len();
                        let after: String = self.buffer.chars().skip(after_offset).collect();
                        let new_len = before.len() + path_str.len();
                        self.buffer = format!("{}{}{}", before, path_str, after).into();
                        self.cursor = new_len;
                    }
                }
                picker.deactivate();
                true
            }
            KeyCode::Esc => {
                let at_pos = self.buffer.rfind('@');
                if let Some(at) = at_pos {
                    let before: String = self.buffer.chars().take(at).collect();
                    let after: String = self
                        .buffer
                        .chars()
                        .skip(at + 1 + picker.query.len())
                        .collect();
                    self.buffer = format!("{}{}", before, after).into();
                    self.cursor = at;
                }
                picker.deactivate();
                true
            }
            _ => false,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<CompactString> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);

        match key.code {
            KeyCode::Enter => {
                if self.picker.as_ref().is_some_and(|p| p.active) {
                    return None;
                }
                // Meta+Enter or Shift+Enter inserts newline
                if has_shift || alt {
                    self.buffer.insert(self.cursor, '\n');
                    self.cursor += 1;
                    self.history_pos = None;
                    return None;
                }
                // Plain Enter → submit
                let text = self.buffer.clone();
                if !text.is_empty() {
                    self.history.push(text.clone());
                }
                self.history_pos = None;
                self.buffer.clear();
                self.cursor = 0;
                self.reset_kill_accumulation();
                if text.is_empty() { None } else { Some(text) }
            }

            // Ctrl+A → start of line
            KeyCode::Char('a') if ctrl => {
                self.cursor = 0;
                self.reset_kill_accumulation();
                None
            }

            // Ctrl+E → end of line
            KeyCode::Char('e') if ctrl => {
                self.cursor = self.buffer.len();
                self.reset_kill_accumulation();
                None
            }

            // Ctrl+B → left one char
            KeyCode::Char('b') if ctrl => {
                if self.cursor > 0 {
                    self.cursor = prev_char_boundary(&self.buffer, self.cursor);
                }
                self.reset_kill_accumulation();
                None
            }

            // Ctrl+F → right one char
            KeyCode::Char('f') if ctrl => {
                if self.cursor < self.buffer.len() {
                    self.cursor = next_char_boundary(&self.buffer, self.cursor);
                }
                self.reset_kill_accumulation();
                None
            }

            // Ctrl+K → kill to end of line
            KeyCode::Char('k') if ctrl => {
                if self.cursor < self.buffer.len() {
                    let killed: CompactString = self.buffer[self.cursor..].into();
                    self.buffer.truncate(self.cursor);
                    self.push_kill(killed, KillDir::Append);
                }
                None
            }

            // Ctrl+U → kill to start of line
            KeyCode::Char('u') if ctrl => {
                if self.cursor > 0 {
                    let killed: CompactString = self.buffer[..self.cursor].into();
                    self.buffer = self.buffer[self.cursor..].into();
                    self.cursor = 0;
                    self.push_kill(killed, KillDir::Prepend);
                }
                None
            }

            // Ctrl+W → kill word before
            KeyCode::Char('w') if ctrl => {
                if self.cursor > 0 {
                    let start = prev_word_boundary(&self.buffer, self.cursor);
                    let killed: CompactString = self.buffer[start..self.cursor].into();
                    self.buffer.replace_range(start..self.cursor, "");
                    self.cursor = start;
                    self.push_kill(killed, KillDir::Prepend);
                }
                None
            }

            // Ctrl+H or Backspace (plain)
            KeyCode::Char('h') if ctrl => {
                if self.cursor > 0 {
                    self.cursor = prev_char_boundary(&self.buffer, self.cursor);
                    self.buffer.remove(self.cursor);
                }
                self.reset_kill_accumulation();
                None
            }

            // Ctrl+Y → yank
            KeyCode::Char('y') if ctrl => {
                if let Some(text) = self.kill_ring.first() {
                    let text = text.clone();
                    let len = text.len();
                    self.buffer.insert_str(self.cursor, &text);
                    self.yank_state = Some(YankState {
                        index: 0,
                        cursor: self.cursor,
                        len,
                    });
                    self.cursor += len;
                }
                self.last_action_was_kill = false;
                None
            }

            // Ctrl+N → history down
            KeyCode::Char('n') if ctrl => {
                self.history_down();
                self.reset_kill_accumulation();
                None
            }

            // Ctrl+P → history up
            KeyCode::Char('p') if ctrl => {
                self.history_up();
                self.reset_kill_accumulation();
                None
            }

            // Meta+Y → yank-pop (cycle kill ring)
            KeyCode::Char('y') if alt => {
                if let Some(ref state) = self.yank_state {
                    let range_end = state.cursor + state.len;
                    if self.kill_ring.len() > 1 && range_end <= self.buffer.len() {
                        let next = (state.index + 1) % self.kill_ring.len();
                        if let Some(text) = self.kill_ring.get(next) {
                            let text = text.clone();
                            self.buffer.replace_range(state.cursor..range_end, "");
                            self.buffer.insert_str(state.cursor, &text);
                            self.cursor = state.cursor + text.len();
                            self.yank_state = Some(YankState {
                                index: next,
                                cursor: state.cursor,
                                len: text.len(),
                            });
                        }
                    }
                }
                None
            }

            // Meta+D → delete word after
            KeyCode::Char('d') if alt => {
                if self.cursor < self.buffer.len() {
                    let end = next_word_boundary(&self.buffer, self.cursor);
                    self.buffer.replace_range(self.cursor..end, "");
                }
                self.reset_kill_accumulation();
                None
            }

            // Meta+B → prev word (Emacs style)
            KeyCode::Char('b') if alt => {
                if self.cursor > 0 {
                    self.cursor = prev_word_boundary(&self.buffer, self.cursor);
                }
                self.reset_kill_accumulation();
                None
            }

            // Meta+F → next word (Emacs style)
            KeyCode::Char('f') if alt => {
                if self.cursor < self.buffer.len() {
                    self.cursor = next_word_boundary(&self.buffer, self.cursor);
                } else {
                    self.cursor = self.buffer.len();
                }
                self.reset_kill_accumulation();
                None
            }

            // Meta+Left → prev word
            KeyCode::Left if alt => {
                if self.cursor > 0 {
                    self.cursor = prev_word_boundary(&self.buffer, self.cursor);
                }
                self.reset_kill_accumulation();
                None
            }

            // Meta+Right → next word
            KeyCode::Right if alt => {
                if self.cursor < self.buffer.len() {
                    self.cursor = next_word_boundary(&self.buffer, self.cursor);
                } else {
                    self.cursor = self.buffer.len();
                }
                self.reset_kill_accumulation();
                None
            }

            // Meta+Backspace → delete word before
            KeyCode::Backspace if alt => {
                if self.cursor > 0 {
                    let start = prev_word_boundary(&self.buffer, self.cursor);
                    self.buffer.replace_range(start..self.cursor, "");
                    self.cursor = start;
                }
                self.reset_kill_accumulation();
                None
            }

            // Plain char: only if not ctrl/alt-modified
            KeyCode::Char(c) if !ctrl && !alt => {
                if c == '@' {
                    let at_word_start = self.cursor == 0
                        || self.buffer.as_bytes().get(self.cursor - 1) == Some(&b' ');
                    if at_word_start {
                        self.start_picker();
                    }
                }
                self.buffer.insert(self.cursor, c);
                self.cursor += c.len_utf8();
                self.history_pos = None;
                self.reset_kill_accumulation();
                None
            }

            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor = prev_char_boundary(&self.buffer, self.cursor);
                    self.buffer.remove(self.cursor);
                }
                self.reset_kill_accumulation();
                None
            }

            KeyCode::Delete => {
                if self.cursor < self.buffer.len() {
                    self.buffer.remove(self.cursor);
                }
                self.reset_kill_accumulation();
                None
            }

            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor = prev_char_boundary(&self.buffer, self.cursor);
                }
                self.reset_kill_accumulation();
                None
            }

            KeyCode::Right => {
                if self.cursor < self.buffer.len() {
                    self.cursor = next_char_boundary(&self.buffer, self.cursor);
                }
                self.reset_kill_accumulation();
                None
            }

            KeyCode::Home => {
                self.cursor = 0;
                self.reset_kill_accumulation();
                None
            }

            KeyCode::End => {
                self.cursor = self.buffer.len();
                self.reset_kill_accumulation();
                None
            }

            KeyCode::Up => {
                self.reset_kill_accumulation();
                // If already navigating history, continue.
                if self.history_pos.is_some() {
                    self.history_up();
                    return None;
                }
                // Try moving up within the multiline buffer first.
                if let Some(pos) = prev_line_start(&self.buffer, self.cursor) {
                    let line_start = cursor_line_start(&self.buffer, self.cursor);
                    let col = self.cursor - line_start;
                    let target_line_end = self.buffer[pos..]
                        .find('\n')
                        .map(|p| pos + p)
                        .unwrap_or(self.buffer.len());
                    self.cursor = (pos + col).min(target_line_end);
                    return None;
                }
                // At top of buffer → fall through to history.
                self.history_up();
                None
            }

            KeyCode::Down => {
                self.reset_kill_accumulation();
                // If already navigating history, continue.
                if self.history_pos.is_some() {
                    self.history_down();
                    return None;
                }
                // Try moving down within the multiline buffer first.
                if let Some(pos) = next_line_start(&self.buffer, self.cursor) {
                    let line_start = cursor_line_start(&self.buffer, self.cursor);
                    let col = self.cursor - line_start;
                    let target_line_end = self.buffer[pos..]
                        .find('\n')
                        .map(|p| pos + p)
                        .unwrap_or(self.buffer.len());
                    self.cursor = (pos + col).min(target_line_end);
                    return None;
                }
                // At bottom of buffer → fall through to history.
                self.history_down();
                None
            }

            KeyCode::Tab => {
                self.buffer.insert_str(self.cursor, "  ");
                self.cursor += 2;
                self.reset_kill_accumulation();
                None
            }

            _ => None,
        }
    }

    fn history_up(&mut self) {
        let hist_len = self.history.len();
        if hist_len == 0 {
            return;
        }
        let pos = match self.history_pos {
            Some(p) if p > 0 => p - 1,
            Some(_) => 0,
            None => hist_len - 1,
        };
        self.history_pos = Some(pos);
        self.buffer = self.history[pos].clone();
        self.cursor = self.buffer.len();
    }

    fn history_down(&mut self) {
        match self.history_pos {
            Some(pos) if pos + 1 < self.history.len() => {
                let new_pos = pos + 1;
                self.history_pos = Some(new_pos);
                self.buffer = self.history[new_pos].clone();
                self.cursor = self.buffer.len();
            }
            Some(_) => {
                self.history_pos = None;
                self.buffer.clear();
                self.cursor = 0;
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prev_word_boundary_basic() {
        assert_eq!(prev_word_boundary("hello world", 11), 6);
        assert_eq!(prev_word_boundary("hello world", 6), 0);
        assert_eq!(prev_word_boundary("hello world", 5), 0);
    }

    #[test]
    fn test_prev_word_from_middle() {
        assert_eq!(prev_word_boundary("hello world", 9), 6); // middle of "world"
    }

    #[test]
    fn test_prev_word_at_start() {
        assert_eq!(prev_word_boundary("hello", 0), 0);
    }

    #[test]
    fn test_prev_word_punctuation() {
        assert_eq!(prev_word_boundary("foo.bar", 7), 4); // start of "bar"
        assert_eq!(prev_word_boundary("foo.bar", 4), 0); // start of "foo"
    }

    #[test]
    fn test_next_word_boundary_basic() {
        // "hello world foo" = 15 bytes
        assert_eq!(next_word_boundary("hello world foo", 0), 6);
        assert_eq!(next_word_boundary("hello world foo", 6), 12);
        assert_eq!(next_word_boundary("hello world foo", 12), 15);
    }

    #[test]
    fn test_next_word_at_end() {
        assert_eq!(next_word_boundary("hello", 5), 5);
    }

    #[test]
    fn test_next_word_punctuation() {
        // With updated logic: from start, skip "foo" + "." → land at "bar_baz" (byte 4)
        assert_eq!(next_word_boundary("foo.bar_baz", 0), 4);
        assert_eq!(next_word_boundary("foo.bar_baz", 3), 4); // from '.', skip it → byte 4
        assert_eq!(next_word_boundary("foo.bar_baz", 4), 11); // skip "bar_baz" → end
    }

    #[test]
    fn test_prev_word_multibyte() {
        // "hå bør": h(0) å(1,2→3) sp(3) b(4) ø(5,6→7) r(7→8)
        assert_eq!(prev_word_boundary("hå bør", 7), 4); // from after 'ø' → start of "bør" at 4
        assert_eq!(prev_word_boundary("hå bør", 4), 0); // from start of "bør" → start of "hå" at 0
    }

    #[test]
    fn test_next_word_multibyte() {
        // "hå bør": 8 bytes. h(0) å(1-2=3) sp(3) b(4) ø(5-6=7) r(7→8)
        assert_eq!(next_word_boundary("hå bør", 0), 4); // skip "hå ", land at "b" (byte 4)
        assert_eq!(next_word_boundary("hå bør", 4), 8); // skip "bør", land at end
    }

    #[test]
    fn test_cursor_line_start() {
        assert_eq!(cursor_line_start("hello\nworld", 10), 6);
        assert_eq!(cursor_line_start("hello\nworld", 3), 0);
        assert_eq!(cursor_line_start("single", 6), 0);
    }

    #[test]
    fn test_prev_line_start() {
        assert_eq!(prev_line_start("hello\nworld", 10), Some(0));
        assert_eq!(prev_line_start("hello\nworld", 3), None);
    }

    #[test]
    fn test_next_line_start() {
        assert_eq!(next_line_start("hello\nworld", 0), Some(6));
        assert_eq!(next_line_start("hello\nworld", 10), None);
    }
}
