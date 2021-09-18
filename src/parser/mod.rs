pub mod transaction;

use nom::{
    IResult,
    combinator::{opt, recognize},
    character::complete::{space0, line_ending},
    sequence::tuple,
};

pub enum LedgerItem<'a> {
    Transaction(transaction::Transaction<'a>),
}

pub struct LedgerParser<'a> {
    s: &'a str,
}

impl<'a> Iterator for LedgerParser<'a> {
    type Item = LedgerItem<'a>;

    fn next(&mut self) -> Option<LedgerItem<'a>> {
        None
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
