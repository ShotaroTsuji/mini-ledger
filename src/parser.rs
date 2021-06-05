use std::borrow::Cow;
use chrono::NaiveDate;
use thiserror::Error;
use nom::IResult;
use nom::branch::alt;
use nom::multi::many0_count;
use nom::combinator::{opt, recognize};
use nom::sequence::tuple;
use nom::bytes::complete::{take_till, take_until, take_while1};
use nom::character::complete::{digit1, char, space1, one_of, none_of};

#[derive(Debug,Error,PartialEq)]
enum ParseError {
    #[error("Invalid date format")]
    DateFormat,
    #[error("Out-of-range date")]
    DateOutOfRange,
    #[error("Invalid beginning line")]
    BeginningLine,
    #[error("Unclosed code")]
    UnclosedCode,
    #[error("Account is missing")]
    MissingAccount,
    #[error("Duplicate unit")]
    DupUnit,
}

#[derive(Debug,Clone,PartialEq)]
pub enum Status {
    Cleared,
    Pending,
    Uncleared,
}

#[derive(Debug,Clone,PartialEq)]
pub struct RawTransaction<'a> {
    date: RawDate<'a>,
    edate: Option<RawDate<'a>>,
    status: Status,
    code: Option<&'a str>,
    description: &'a str,
    comment: &'a str,
}

#[derive(Debug,Clone,PartialEq)]
pub struct RawAmount<'a> {
    price: &'a str,
    unit: &'a str,
}

impl<'a> RawAmount<'a> {
    pub fn from_str(price: &'a str, unit: &'a str) -> Self {
        Self {
            price: price,
            unit: unit,
        }
    }
}

#[derive(Debug,Clone,PartialEq)]
pub struct RawPosting<'a> {
    account: &'a str,
    amount: Option<RawAmount<'a>>,
    assign: Option<RawAmount<'a>>,
    comment: &'a str,
}

#[derive(Debug,Clone,PartialEq)]
pub struct RawDate<'a> {
    year: &'a str,
    month: &'a str,
    day: &'a str,
}

impl<'a> RawDate<'a> {
    pub fn from_str(y: &'a str, m: &'a str, d: &'a str) -> Self {
        RawDate {
            year: y,
            month: m,
            day: d,
        }
    }
}

fn date_slash(input: &str) -> IResult<&str, (&str, char, &str, char, &str)> {
    tuple(( digit1, char('/'), digit1, char('/'), digit1 ))(input)
}

fn date_dash(input: &str) -> IResult<&str, (&str, char, &str, char, &str)> {
    tuple(( digit1, char('-'), digit1, char('-'), digit1 ))(input)
}

pub fn date(input: &str) -> IResult<&str, RawDate> {
    let (input, date) = alt((date_slash, date_dash))(input)?;
    let date = RawDate {
        year: date.0,
        month: date.2,
        day: date.4,
    };
    Ok((input, date))
}

fn status(input: &str) -> IResult<&str, Status> {
    let (input, (_, result)) = tuple((space1, one_of("!*")))(input)?;
    let result = match result {
        '*' => Status::Cleared,
        '!' => Status::Pending,
        _ => unreachable!(),
    };
    Ok((input, result))
}

fn code(input: &str) -> IResult<&str, &str> {
    let (input, result) = tuple((space1, char('('), take_until(")"), char(')')))(input)?;
    Ok((input, result.2))
}

pub fn transaction_header(input: &str) -> IResult<&str, RawTransaction> {
    let (input, r_date) = date(input)?;
    let (input, edate) = opt(tuple((char('='), date)))(input)?;
    let (input, status) = opt(status)(input)?;
    let (input, code) = opt(code)(input)?;
    let (input, _) = space1(input)?;
    let (input, desc) = take_till(|c: char| c == ';')(input)?;
    let comment = input;

    let trans = RawTransaction {
        date: r_date,
        edate: edate.map(|(_, d)| d),
        status: status.unwrap_or(Status::Uncleared),
        code: code,
        description: desc,
        comment: comment,
    };

    Ok(("", trans))
}

fn account(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| !c.is_ascii_whitespace())(input)
}

fn decimal(input: &str) -> IResult<&str, &str> {
    recognize(tuple(
            (opt(one_of("+-")),
             digit1,
             many0_count(tuple((char(','), digit1))),
             opt(tuple((char('.'), digit1)))
            )
    ))(input)
}

fn amount_dollar(input: &str) -> IResult<&str, RawAmount> {
    let (input, result) = tuple((space1, char('$'), decimal))(input)?;

    Ok((input, RawAmount {
        price: result.2,
        unit: "$",
    }))
}

fn unit(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| !c.is_ascii_whitespace())(input)
}

fn amount_unit(input: &str) -> IResult<&str, RawAmount> {
    let (input, result) = tuple((space1, decimal, space1, unit))(input)?;

    Ok((input, RawAmount {
        price: result.1,
        unit: result.3,
    }))
}

fn assign_amount(input: &str) -> IResult<&str, RawAmount> {
    let (input, _) = space1(input)?;
    let (input, _) = char('=')(input)?;
    let (input, _) = space1(input)?;
    let (input, price) = decimal(input)?;
    let (input, result) = opt(tuple((space1, unit)))(input)?;

    Ok((input, RawAmount {
        price: price,
        unit: result.map(|x| x.1).unwrap_or(""),
    }))
}

pub fn posting(input: &str) -> IResult<&str, RawPosting> {
    let (input, _) = space1(input)?;
    let (input, account) = account(input)?;
    let (input, amount) = opt(alt((amount_dollar, amount_unit)))(input)?;
    let (input, assign) = opt(assign_amount)(input)?;
    let (input, remain) = opt(take_until(";"))(input)?;

    let comment = match remain {
        Some(_) => input,
        None => "",
    };

    let posting = RawPosting {
        account: account,
        amount: amount,
        assign: assign,
        comment: comment,
    };

    Ok(("", posting))
}

pub fn is_transaction_header(input: &str) -> bool {
    match input.chars().nth(0) {
        Some(c) if c.is_ascii_digit() => true,
        _ => false,
    }
}

pub fn is_posting(input: &str) -> bool {
    let result = tuple::<_, _, (), _>((space1, none_of(";")))(input);
    result.is_ok()
}

pub fn is_posting_comment(input: &str) -> bool {
    let result = tuple::<_, _, (), _>((space1, one_of(";")))(input);
    result.is_ok()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn date_ok() {
        assert_eq!(date_dash("2020-11-20"), Ok(("", ("2020", '-', "11", '-', "20"))));
        assert_eq!(date_slash("2020/11/20"), Ok(("", ("2020", '/', "11", '/', "20"))));
        assert_eq!(
            date("2020-11-30 * Withdraw"),
            Ok((" * Withdraw", RawDate {
                year: "2020",
                month: "11",
                day: "30",
            }))
        );
    }

    #[test]
    fn decimal_ok() {
        assert_eq!(decimal("1000"), Ok(("", "1000")));
        assert_eq!(decimal("-9900"), Ok(("", "-9900")));
        assert_eq!(decimal("+10.49"), Ok(("", "+10.49")));
        assert_eq!(decimal("24,000"), Ok(("", "24,000")));
        assert_eq!(decimal("12,345.992"), Ok(("", "12,345.992")));
    }

    #[test]
    fn trans_header_ok() {
        assert_eq!(
            transaction_header("2020-11-30 * Withdraw"),
            Ok(("", RawTransaction {
                date: RawDate::from_str("2020", "11", "30"),
                edate: None,
                status: Status::Cleared,
                code: None,
                description: "Withdraw",
                comment: "",
            }))
        );
        assert_eq!(
            transaction_header("2020-11-30 ! Withdraw   "),
            Ok(("", RawTransaction {
                date: RawDate::from_str("2020", "11", "30"),
                edate: None,
                status: Status::Pending,
                code: None,
                description: "Withdraw   ",
                comment: "",
            }))
        );
        assert_eq!(
            transaction_header("2020-11-30 Withdraw ; comment"),
            Ok(("", RawTransaction {
                date: RawDate::from_str("2020", "11", "30"),
                edate: None,
                status: Status::Uncleared,
                code: None,
                description: "Withdraw ",
                comment: "; comment",
            }))
        );
    }

    #[test]
    fn trans_header_edate_ok() {
        assert_eq!(
            transaction_header("2020-11-30=2020-12-14 * Withdraw"),
            Ok(("", RawTransaction {
                date: RawDate::from_str("2020", "11", "30"),
                edate: Some(RawDate::from_str("2020", "12", "14")),
                status: Status::Cleared,
                code: None,
                description: "Withdraw",
                comment: "",
            }))
        );
    }

    #[test]
    fn trans_header_code_ok() {
        assert_eq!(
            transaction_header("2020-11-30 * (#100) Withdraw"),
            Ok(("", RawTransaction {
                date: RawDate::from_str("2020", "11", "30"),
                edate: None,
                status: Status::Cleared,
                code: Some("#100"),
                description: "Withdraw",
                comment: "",
            }))
        );
    }

    #[test]
    fn normal_posting() {
        assert_eq!(
            posting("    Assets:Cash $100.05"),
            Ok(("", RawPosting {
                account: "Assets:Cash",
                amount: Some(RawAmount::from_str("100.05", "$")),
                assign: None,
                comment: "",
            }))
        );
        assert_eq!(
            posting("    Assets:Cash 3000 JPY   "),
            Ok(("", RawPosting {
                account: "Assets:Cash",
                amount: Some(RawAmount::from_str("3000", "JPY")),
                assign: None,
                comment: "",
            }))
        );
        assert_eq!(
            posting("    Liabilities:CreditCard -3000 JPY ; comment"),
            Ok(("", RawPosting {
                account: "Liabilities:CreditCard",
                amount: Some(RawAmount::from_str("-3000", "JPY")),
                assign: None,
                comment: "; comment",
            }))
        );
    }

    #[test]
    fn assign_ok() {
        assert_eq!(
            assign_amount(" = 3000 JPY"),
            Ok(("", RawAmount::from_str("3000", "JPY")))
        );
        assert_eq!(
            assign_amount(" = 0"),
            Ok(("", RawAmount::from_str("0", "")))
        );
    }

    #[test]
    fn assign_posting() {
        assert_eq!(
            posting("    Assets:Cash    500 JPY = 3000 JPY"),
            Ok(("", RawPosting {
                account: "Assets:Cash",
                amount: Some(RawAmount::from_str("500", "JPY")),
                assign: Some(RawAmount::from_str("3000", "JPY")),
                comment: "",
            }))
        );
    }

    #[test]
    fn elided_posting() {
        assert_eq!(
            posting("    Assets:Cash"),
            Ok(("", RawPosting {
                account: "Assets:Cash",
                amount: None,
                assign: None,
                comment: "",
            }))
        );
    }

    #[test]
    fn predicates() {
        assert_eq!(is_transaction_header("2020-10-05 * Withdraw"), true);
        assert_eq!(is_posting("2020-10-05 * Withdraw"), false);
        assert_eq!(is_posting_comment("2020-10-05 * Withdraw"), false);

        assert_eq!(is_transaction_header("    Liabilities:CreditCard"), false);
        assert_eq!(is_posting("    Liabilities:CreditCard"), true);
        assert_eq!(is_posting_comment("    Liabilities:CreditCard"), false);

        assert_eq!(is_transaction_header("    ; comment"), false);
        assert_eq!(is_posting("    ; comment"), false);
        assert_eq!(is_posting_comment("    ; comment"), true);
    }
}
