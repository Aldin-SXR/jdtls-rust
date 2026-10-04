//! Fresh-document partitioning from AbstractFastJavaPartitionScanner and
//! FastPartitioner.getPartition(offset, preferOpenPartitions). JDT LS creates
//! a new partitioner for each smart-semicolon request, so there is no
//! incremental position cache to emulate here.

use super::whitespace;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Java,
    LineComment,
    BlockComment,
    Javadoc,
    Character,
    String,
    TextBlock,
    Markdown,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Region {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

pub(super) struct Partitions {
    regions: Vec<Region>,
    length: usize,
}

impl Partitions {
    pub fn new(text: &[u16], text_blocks: bool) -> Self {
        let mut scanner = Scanner {
            text,
            text_blocks,
            cursor: 0,
            offset: 0,
            length: 0,
            prefix_length: 0,
            state: Kind::Java,
            last: Last::None,
        };
        let mut regions = Vec::new();
        while let Some(region) = scanner.next() {
            // installJavaStuff's legal types omit Markdown. FastPartitioner
            // leaves unsupported scanner tokens in a default Java gap.
            if !matches!(region.kind, Kind::Java | Kind::Markdown) {
                regions.push(region);
            }
        }
        Self {
            regions,
            length: text.len(),
        }
    }

    pub fn at(&self, offset: usize, prefer_open: bool) -> Region {
        let index = self.regions.partition_point(|r| r.end <= offset);
        let start = index.checked_sub(1).map_or(0, |i| self.regions[i].end);
        let next = self.regions.get(index);
        if let Some(region) = next.filter(|r| r.start <= offset) {
            if prefer_open && offset == region.start {
                // Only a non-empty default partition to the left is open.
                return Region {
                    start,
                    end: offset,
                    kind: Kind::Java,
                };
            }
            return *region;
        }
        Region {
            start,
            end: next.map_or(self.length, |r| r.start),
            kind: Kind::Java,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Last {
    None,
    Slash,
    Star,
    SlashSlash,
    SlashStar,
    SlashStarStar,
    TripleSlash,
    TripleQuote,
    Backslash,
    Cr,
}
impl Last {
    fn length(self) -> usize {
        match self {
            Self::None => 0,
            Self::Slash | Self::Star | Self::Backslash | Self::Cr => 1,
            Self::SlashSlash | Self::SlashStar => 2,
            Self::SlashStarStar | Self::TripleSlash | Self::TripleQuote => 3,
        }
    }
}

struct Scanner<'a> {
    text: &'a [u16],
    text_blocks: bool,
    cursor: usize,
    offset: usize,
    length: usize,
    prefix_length: usize,
    state: Kind,
    last: Last,
}
impl Scanner<'_> {
    fn region(&self, kind: Kind) -> Region {
        Region {
            start: self.offset,
            end: self.offset + self.length,
            kind,
        }
    }
    fn consume(&mut self) {
        self.length += 1;
        self.last = Last::None;
    }
    fn postfix(&mut self, kind: Kind) -> Region {
        self.consume();
        self.state = Kind::Java;
        self.prefix_length = 0;
        self.region(kind)
    }
    fn prefix(&mut self, state: Kind, last: Last, length: usize) -> Region {
        let old = self.state;
        self.length -= self.last.length();
        self.last = last;
        self.prefix_length = length;
        self.state = state;
        self.region(old)
    }
    fn begin(&mut self, state: Kind, last: Last, length: usize) -> Option<Region> {
        let remaining = self.length - self.last.length();
        let region = self.prefix(state, last, length);
        if remaining > 0 {
            Some(region)
        } else {
            self.offset += self.length;
            self.length = self.prefix_length;
            None
        }
    }
    fn unicode_slash(&mut self) {
        if self.text.get(self.cursor..self.cursor + 5) == Some(&[117, 48, 48, 53, 67]) {
            self.cursor += 5;
            self.length += 5;
        }
    }
    fn text_block_beginning(&self) -> bool {
        if !self.text_blocks || self.text.get(self.cursor..self.cursor + 2) != Some(&[34, 34]) {
            return false;
        }
        for &c in &self.text[self.cursor + 2..] {
            if matches!(c, 10 | 13) {
                return true;
            }
            if !whitespace(c) {
                return false;
            }
        }
        false
    }
    fn next(&mut self) -> Option<Region> {
        self.offset += self.length;
        self.length = self.prefix_length;
        loop {
            let Some(&ch) = self.text.get(self.cursor) else {
                self.last = Last::None;
                return if self.length > 0 {
                    Some(self.prefix(Kind::Java, Last::None, 0))
                } else {
                    None
                };
            };
            self.cursor += 1;
            // Newlines are handled before the state-specific transitions.
            if ch == 13 {
                if self.last != Last::Cr {
                    self.last = Last::Cr;
                    self.length += 1;
                    continue;
                }
                if matches!(
                    self.state,
                    Kind::Markdown | Kind::LineComment | Kind::Character | Kind::String
                ) && self.length > 0
                {
                    let region = self.region(self.state);
                    self.last = Last::Cr;
                    self.prefix_length = 1;
                    self.state = Kind::Java;
                    return Some(region);
                }
                self.consume();
                continue;
            }
            if ch == 10 {
                if matches!(
                    self.state,
                    Kind::Markdown | Kind::LineComment | Kind::Character | Kind::String
                ) {
                    return Some(self.postfix(self.state));
                }
                self.consume();
                continue;
            }
            if self.last == Last::Cr
                && matches!(
                    self.state,
                    Kind::LineComment | Kind::Character | Kind::String
                )
            {
                // A lone CR ends these partitions, and the consumed next
                // character becomes the prefix of a new token. Markdown is
                // intentionally absent from this upstream branch.
                let (state, last) = match ch {
                    47 => (Kind::Java, Last::Slash),
                    42 => (Kind::Java, Last::Star),
                    39 => (Kind::Character, Last::None),
                    34 => (Kind::String, Last::None),
                    92 => (Kind::Java, Last::Backslash),
                    _ => (Kind::Java, Last::None),
                };
                self.last = Last::None;
                return Some(self.prefix(state, last, 1));
            }
            match self.state {
                Kind::Java => match ch {
                    47 if self.last == Last::Slash => {
                        if let Some(r) = self.begin(Kind::LineComment, Last::SlashSlash, 2) {
                            return Some(r);
                        }
                    }
                    47 => {
                        self.length += 1;
                        self.last = Last::Slash;
                    }
                    42 if self.last == Last::Slash => {
                        if let Some(r) = self.begin(Kind::BlockComment, Last::SlashStar, 2) {
                            return Some(r);
                        }
                    }
                    39 => {
                        self.last = Last::None;
                        if let Some(r) = self.begin(Kind::Character, Last::None, 1) {
                            return Some(r);
                        }
                    }
                    34 => {
                        if self.text_block_beginning() {
                            self.cursor += 2;
                            if let Some(r) = self.begin(Kind::TextBlock, Last::TripleQuote, 3) {
                                return Some(r);
                            }
                        } else {
                            self.last = Last::None;
                            if let Some(r) = self.begin(Kind::String, Last::None, 1) {
                                return Some(r);
                            }
                        }
                    }
                    _ => self.consume(),
                },
                Kind::LineComment => {
                    if ch == 47 {
                        self.length += 1;
                        if self.last == Last::SlashSlash {
                            self.last = Last::TripleSlash;
                            self.state = Kind::Markdown;
                        } else {
                            self.last = Last::Slash;
                        }
                    } else {
                        self.consume();
                    }
                }
                Kind::Markdown => self.consume(),
                Kind::Javadoc => match ch {
                    47 if self.last == Last::SlashStarStar => {
                        return Some(self.postfix(Kind::BlockComment))
                    }
                    47 if self.last == Last::Star => return Some(self.postfix(Kind::Javadoc)),
                    42 => {
                        self.length += 1;
                        self.last = Last::Star;
                    }
                    _ => self.consume(),
                },
                Kind::BlockComment => match ch {
                    42 => {
                        self.length += 1;
                        if self.last == Last::SlashStar {
                            self.last = Last::SlashStarStar;
                            self.state = Kind::Javadoc;
                        } else {
                            self.last = Last::Star;
                        }
                    }
                    47 if self.last == Last::Star => return Some(self.postfix(Kind::BlockComment)),
                    _ => self.consume(),
                },
                Kind::String | Kind::Character | Kind::TextBlock => {
                    if ch == 92 {
                        self.last = if self.last == Last::Backslash {
                            Last::None
                        } else {
                            Last::Backslash
                        };
                        self.length += 1;
                        self.unicode_slash();
                    } else if self.state == Kind::TextBlock {
                        if ch == 34
                            && self.last != Last::Backslash
                            && self.text.get(self.cursor..self.cursor + 2) == Some(&[34, 34])
                        {
                            self.cursor += 2;
                            self.length += 2;
                            return Some(self.postfix(Kind::TextBlock));
                        }
                        self.consume();
                    } else if ch == if self.state == Kind::String { 34 } else { 39 }
                        && self.last != Last::Backslash
                    {
                        return Some(self.postfix(self.state));
                    } else {
                        self.consume();
                    }
                }
            }
        }
    }
}
