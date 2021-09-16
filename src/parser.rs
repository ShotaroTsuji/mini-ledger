use chrono::NaiveDate;
use nom::branch::alt;
use nom::bytes::complete::{take_while, take_until, take_while1, tag};
use nom::character::complete::{char, digit1, one_of, space0, space1};
use nom::combinator::{map, map_res, opt, recognize};
use nom::multi::{many0_count, many1};
use nom::sequence::{preceded, tuple};
use nom::IResult;
use rust_decimal::Decimal;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum ParseError {
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

#[derive(Debug, Clone, PartialEq)]
pub struct Transaction<'a> {
    header: TransactionHeader<'a>,
    posting: Vec<Posting<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Cleared,
    Pending,
    Uncleared,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransactionHeader<'a> {
    date: NaiveDate,
    edate: Option<NaiveDate>,
    status: Status,
    code: Option<&'a str>,
    description: &'a str,
    comment: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Amount<'a> {
    price: Decimal,
    unit: &'a str,
}

impl<'a> Amount<'a> {
    pub fn from_str(price: &'a str, unit: &'a str) -> Result<Self, rust_decimal::Error> {
        Ok(Self {
            price: price.parse()?,
            unit: unit,
        })
    }

    pub fn dollar(price: &'a str) -> Result<Self, rust_decimal::Error> {
        Self::from_str(price, "$")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Posting<'a> {
    account: &'a str,
    amount: Option<Amount<'a>>,
    assign: Option<Amount<'a>>,
    cost: Option<Amount<'a>>,
    comment: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawDate<'a> {
    pub year: &'a str,
    pub month: &'a str,
    pub day: &'a str,
}

impl<'a> RawDate<'a> {
    pub fn from_ymd(y: &'a str, m: &'a str, d: &'a str) -> Self {
        RawDate {
            year: y,
            month: m,
            day: d,
        }
    }

    pub fn from_triple(t: (&'a str, &'a str, &'a str)) -> Self {
        Self::from_ymd(t.0, t.1, t.2)
    }

    pub fn into_naive_date(self) -> Result<NaiveDate, ParseError> {
        let year: i32 = self.year.parse().unwrap();
        let month: u32 = self.month.parse().unwrap();
        let day: u32 = self.day.parse().unwrap();

        NaiveDate::from_ymd_opt(year, month, day)
            .ok_or(ParseError::DateOutOfRange)
    }
}

// Parses a date separated with slashes like `2021/09/07`.
fn date_slash(input: &str) -> IResult<&str, (&str, &str, &str)> {
    map(
        tuple((digit1, char('/'), digit1, char('/'), digit1)),
        |(y, _, m, _, d)| (y, m, d),
    )(input)
}

// Parses a date separated with hyphens like `2021-09-07`.
fn date_dash(input: &str) -> IResult<&str, (&str, &str, &str)> {
    map(
        tuple((digit1, char('-'), digit1, char('-'), digit1)),
        |(y, _, m, _, d)| (y, m, d),
    )(input)
}

/// Parses transaction date
fn date(input: &str) -> IResult<&str, NaiveDate> {
    map_res(
        alt((date_slash, date_dash)),
        |t| RawDate::from_triple(t).into_naive_date()
    )(input)
}

// Parses transaction status
fn status(input: &str) -> IResult<&str, Status> {
    map(one_of("!*"), |c| match c {
        '*' => Status::Cleared,
        '!' => Status::Pending,
        _ => unreachable!(),
    })(input)
}

// Parses transaction code
//
// A transaction code is a code delimited by parentheses.
fn code(input: &str) -> IResult<&str, &str> {
    map(
        tuple((char('('), take_until(")"), char(')'))),
        |(_, code, _)| code,
    )(input)
}

fn comment(input: &str) -> IResult<&str, &str> {
    preceded(
        tuple((char(';'), space0)),
        take_while(|c| c != '\n')
    )(input)
}

pub fn transaction_header(input: &str) -> IResult<&str, TransactionHeader> {
    map(
        tuple((
            date,
            opt(preceded(char('='), date)),
            opt(preceded(space1, status)),
            opt(preceded(space1, code)),
            space1,
            take_while(|c: char| c != ';' && c != '\n'),
            opt(comment),
            opt(char('\n'))
        )),
        |(date, edate, status, code, _, desc, comment, _)| TransactionHeader {
            date: date,
            edate: edate,
            status: status.unwrap_or(Status::Uncleared),
            code: code,
            description: desc,
            comment: comment,
        },
    )(input)
}

// Parses an account name
fn account(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| !c.is_ascii_whitespace())(input)
}

// Parses a decimal value without sign
fn unsigned_decimal(input: &str) -> IResult<&str, &str> {
    recognize(
        tuple((
            digit1,
            many0_count(digit1),
            opt(tuple((char('.'), digit1)))
        ))
    )(input)
}

// Parses a decimal value
fn decimal(input: &str) -> IResult<&str, &str> {
    recognize(tuple((
        opt(one_of("+-")),
        unsigned_decimal,
    )))(input)
}

fn is_unit_char(c: char) -> bool {
    !c.is_whitespace() &&
        !c.is_ascii_digit() &&
        !".,;:?!-+*/^&|=<>[](){}@".contains(c)
}

/// Parses a commodity unit
/// 
/// TODO: support quoted units
fn unit(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| is_unit_char(c))(input)
}

/// Parses amount with arbitrary unit like `1000 JPY`.
fn amount_unit(input: &str) -> IResult<&str, Amount> {
    map_res(
        tuple((decimal, opt(preceded(space1, unit)))),
        |(price, unit)| Amount::from_str(price, unit.unwrap_or(""))
    )(input)
}

fn assign_amount(input: &str) -> IResult<&str, Amount> {
    map(
        tuple((char('='), space0, amount_unit)),
        |(_, _, amount)| amount
    )(input)
}

fn cost(input: &str) -> IResult<&str, Amount> {
    preceded(
        tuple((char('@'), space0)),
        amount_unit
    )(input)
}

fn posting_indent(input: &str) -> IResult<&str, &str> {
    preceded(
        alt((tag("  "), tag("\t"))),
        space0
    )(input)
}

pub fn posting(input: &str) -> IResult<&str, Posting> {
    map(
        tuple((
                posting_indent,
                account,
                space0,
                opt(amount_unit),
                space0,
                opt(assign_amount),
                space0,
                opt(cost),
                space0,
                opt(comment),
                opt(char('\n'))
        )),
        |(_, account, _, amount, _, assign, _, cost, _, comment, _)| Posting {
            account: account,
            amount: amount,
            assign: assign,
            cost: cost,
            comment: comment,
        }
    )(input)
}

pub fn transaction(input: &str) -> IResult<&str, Transaction> {
    map(
        tuple((
            transaction_header,
            many1(posting),
        )),
        |(header, posting)| Transaction {
            header: header,
            posting: posting,
        }
    )(input)
}

#[cfg(test)]
mod test {
    use super::*;

    fn parse_assert_eq<'a, T, F>(mut f: F, s: &'a str, expected: (&str, T))
        where
            F: FnMut(&'a str) -> IResult<&'a str, T>,
            T: PartialEq + std::fmt::Debug,
    {
        assert_eq!(f(s), Ok(expected));
    }

    #[test]
    fn parse_date() {
        vec![
            ("2021/12/23", "", NaiveDate::from_ymd(2021, 12, 23)),
            ("2020/05/23", "", NaiveDate::from_ymd(2020, 05, 23)),
            ("2020-01-04", "", NaiveDate::from_ymd(2020, 01, 04)),
        ]
            .into_iter()
            .for_each(|(s, r, e)| parse_assert_eq(date, s, (r, e)));
    }

    #[test]
    fn parse_code() {
        assert_eq!(code("(302)"), Ok(("", "302")));
    }

    #[test]
    fn parse_simple_transaction_header() {
        assert_eq!(
            transaction_header("2020-11-30 * Withdraw\n    "),
            Ok((
                "    ",
                TransactionHeader {
                    date: NaiveDate::from_ymd(2020, 11, 30),
                    edate: None,
                    status: Status::Cleared,
                    code: None,
                    description: "Withdraw",
                    comment: None,
                }
            ))
        );
        assert_eq!(
            transaction_header("2020-11-30 ! Withdraw   \n"),
            Ok((
                "",
                TransactionHeader {
                    date: NaiveDate::from_ymd(2020, 11, 30),
                    edate: None,
                    status: Status::Pending,
                    code: None,
                    description: "Withdraw   ",
                    comment: None,
                }
            ))
        );
        assert_eq!(
            transaction_header("2020-11-30 Withdraw ; comment\n"),
            Ok((
                "",
                TransactionHeader {
                    date: NaiveDate::from_ymd(2020, 11, 30),
                    edate: None,
                    status: Status::Uncleared,
                    code: None,
                    description: "Withdraw ",
                    comment: Some("comment"),
                }
            ))
        );
    }

    #[test]
    fn parse_transaction_header_with_edate() {
        assert_eq!(
            transaction_header("2020-11-30=2020-12-14 * Withdraw"),
            Ok((
                "",
                TransactionHeader {
                    date: NaiveDate::from_ymd(2020, 11, 30),
                    edate: Some(NaiveDate::from_ymd(2020, 12, 14)),
                    status: Status::Cleared,
                    code: None,
                    description: "Withdraw",
                    comment: None,
                }
            ))
        );
    }

    #[test]
    fn parse_transaction_header_with_code() {
        assert_eq!(
            transaction_header("2020-11-30 * (#100) Withdraw"),
            Ok((
                "",
                TransactionHeader {
                    date: NaiveDate::from_ymd(2020, 11, 30),
                    edate: None,
                    status: Status::Cleared,
                    code: Some("#100"),
                    description: "Withdraw",
                    comment: None,
                }
            ))
        );
    }

    #[test]
    fn parse_transaction_header_with_full_options() {
        assert_eq!(
            transaction_header("2020-11-30=2020-12-11 * (#100) Withdraw ; modified\n    Assets"),
            Ok((
                "    Assets",
                TransactionHeader {
                    date: NaiveDate::from_ymd(2020, 11, 30),
                    edate: Some(NaiveDate::from_ymd(2020, 12, 11)),
                    status: Status::Cleared,
                    code: Some("#100"),
                    description: "Withdraw ",
                    comment: Some("modified"),
                }
            ))
        );
    }

    #[test]
    fn parse_decimal_values() {
        assert_eq!(decimal("1000"), Ok(("", "1000")));
        assert_eq!(decimal("-9900"), Ok(("", "-9900")));
        assert_eq!(decimal("+10.49"), Ok(("", "+10.49")));
    }

    #[test]
    fn parse_plain_amount() {
        assert_eq!(amount_unit("0"), Ok(("", Amount::from_str("0", "").unwrap())));
        assert_eq!(amount_unit("11.0"), Ok(("", Amount::from_str("11.0", "").unwrap())));
    }

    #[test]
    fn parse_unit_amount() {
        assert_eq!(
            amount_unit("320 JPY"),
            Ok(("", Amount::from_str("320", "JPY").unwrap()))
        );
        assert_eq!(
            amount_unit("-12.5 JPY"),
            Ok(("", Amount::from_str("-12.5", "JPY").unwrap()))
        );
        assert_eq!(
            amount_unit("1000 VTI"),
            Ok(("", Amount::from_str("1000", "VTI").unwrap()))
        );
    }

    #[test]
    fn parse_assign_amount() {
        assert_eq!(
            assign_amount("= 100 JPY"),
            Ok(("", Amount::from_str("100", "JPY").unwrap()))
        );
        assert_eq!(
            assign_amount("= 0"),
            Ok(("", Amount::from_str("0", "").unwrap()))
        );
    }

    #[test]
    fn parse_normal_posting() {
        assert_eq!(
            posting("    Assets:Cash 100.05 EUR\n"),
            Ok((
                "",
                Posting {
                    account: "Assets:Cash",
                    amount: Some(Amount::from_str("100.05", "EUR").unwrap()),
                    assign: None,
                    cost: None,
                    comment: None,
                }
            ))
        );
        assert_eq!(
            posting("    Assets:Cash 3000 JPY   "),
            Ok((
                "",
                Posting {
                    account: "Assets:Cash",
                    amount: Some(Amount::from_str("3000", "JPY").unwrap()),
                    assign: None,
                    cost: None,
                    comment: None,
                }
            ))
        );
        assert_eq!(
            posting("    Liabilities:CreditCard -3000 JPY ; comment"),
            Ok((
                "",
                Posting {
                    account: "Liabilities:CreditCard",
                    amount: Some(Amount::from_str("-3000", "JPY").unwrap()),
                    assign: None,
                    cost: None,
                    comment: Some("comment"),
                }
            ))
        );
    }

    #[test]
    fn parse_assign_posting() {
        assert_eq!(
            posting("    Assets:Cash    500 JPY = 3000 JPY\n"),
            Ok((
                "",
                Posting {
                    account: "Assets:Cash",
                    amount: Some(Amount::from_str("500", "JPY").unwrap()),
                    assign: Some(Amount::from_str("3000", "JPY").unwrap()),
                    cost: None,
                    comment: None,
                }
            ))
        );
        assert_eq!(
            posting("    Assets:Cash    =0 ; balance the cash\n"),
            Ok((
                "",
                Posting {
                    account: "Assets:Cash",
                    amount: None,
                    assign: Some(Amount::from_str("0", "").unwrap()),
                    cost: None,
                    comment: Some("balance the cash"),
                }
            ))
        );
    }

    #[test]
    fn parse_posting_with_cost() {
        assert_eq!(
            posting("    Assets:ETF     1 VTI @ 12300 JPY\n"),
            Ok((
                "",
                Posting {
                    account: "Assets:ETF",
                    amount: Some(Amount::from_str("1", "VTI").unwrap()),
                    assign: None,
                    cost: Some(Amount::from_str("12300", "JPY").unwrap()),
                    comment: None,
                }
            ))
        );
    }

    #[test]
    fn parse_elided_posting() {
        assert_eq!(
            posting("    Assets:Cash"),
            Ok((
                "",
                Posting {
                    account: "Assets:Cash",
                    amount: None,
                    assign: None,
                    cost: None,
                    comment: None,
                }
            ))
        );
    }

    #[test]
    fn parse_posting_without_indent() {
        assert!(posting("Assets:Cash").is_err());
    }

    #[test]
    fn parse_simple_transaction() {
        let s = r#"2021-09-16 * 引き出し
    資産:現金           1000 JPY
    資産:普通預金:JP    -1000 JPY"#;
        assert_eq!(
            transaction(s),
            Ok(("", Transaction {
                header: TransactionHeader {
                    date: NaiveDate::from_ymd(2021, 9, 16),
                    edate: None,
                    status: Status::Cleared,
                    code: None,
                    description: "引き出し",
                    comment: None,
                },
                posting: vec![
                    Posting {
                        account: "資産:現金",
                        amount: Amount::from_str("1000", "JPY").ok(),
                        assign: None,
                        cost: None,
                        comment: None,
                    },
                    Posting {
                        account: "資産:普通預金:JP",
                        amount: Amount::from_str("-1000", "JPY").ok(),
                        assign: None,
                        cost: None,
                        comment: None,
                    },
                ],
            }))
        );
    }

    #[test]
    fn parse_transaction_with_three_postings() {
        let s = r#"2021-09-20 * Tomod's
    費用:食費           500 JPY
    費用:消耗品費       1000 JPY
    資産:現金
"#;
        assert_eq!(
            transaction(s),
            Ok(("", Transaction {
                header: TransactionHeader {
                    date: NaiveDate::from_ymd(2021, 9, 20),
                    edate: None,
                    status: Status::Cleared,
                    code: None,
                    description: "Tomod's",
                    comment: None,
                },
                posting: vec![
                    Posting {
                        account: "費用:食費",
                        amount: Amount::from_str("500", "JPY").ok(),
                        assign: None,
                        cost: None,
                        comment: None,
                    },
                    Posting {
                        account: "費用:消耗品費",
                        amount: Amount::from_str("1000", "JPY").ok(),
                        assign: None,
                        cost: None,
                        comment: None,
                    },
                    Posting {
                        account: "資産:現金",
                        amount: None,
                        assign: None,
                        cost: None,
                        comment: None,
                    },
                ],
            }))
        );
    }

}
