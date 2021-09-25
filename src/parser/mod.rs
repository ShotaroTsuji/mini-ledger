pub mod transaction;

use nom::{
    IResult,
    combinator::{opt, recognize},
    character::complete::{space0, line_ending},
    sequence::tuple,
};

#[derive(Debug,PartialEq)]
pub enum LedgerItem<'a> {
    Transaction(transaction::Transaction<'a>),
    Blank,
}

pub struct LedgerParser<'a> {
    s: &'a str,
}

impl<'a> LedgerParser<'a> {
    pub fn new(s: &'a str) -> Self {
        Self {
            s: s,
        }
    }
}

impl<'a> Iterator for LedgerParser<'a> {
    type Item = LedgerItem<'a>;

    fn next(&mut self) -> Option<LedgerItem<'a>> {
        if self.s.is_empty() {
            return None;
        }

        let (remain, ret) = if self.s.starts_with(|c: char| c.is_ascii_digit()) {
            let t = transaction::transaction(self.s).unwrap();
            (t.0, LedgerItem::Transaction(t.1))
        } else {
            let (remain, _) = blank_line(self.s).unwrap();
            (remain, LedgerItem::Blank)
        };

        self.s = remain;

        Some(ret)
    }
}

pub fn blank_line(input: &str) -> IResult<&str, &str> {
    recognize(
        tuple((space0, opt(line_ending)))
    )(input)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_blank_line() {
        assert_eq!(blank_line("\n"), Ok(("", "\n")));
        assert_eq!(blank_line("  \n"), Ok(("", "  \n")));
        assert_eq!(blank_line("\t\t\n2020"), Ok(("2020", "\t\t\n")));
    }
}
