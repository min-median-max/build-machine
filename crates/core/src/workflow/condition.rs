//! A step's `if:` read as GitHub reads it.
//!
//! Only what is knowable on this machine is accepted: the job status functions,
//! boolean literals, `!`, `&&`, `||` and parentheses. A context such as
//! `github.ref` or `steps.x.outputs.y` has no local value, so a condition that
//! reads one fails validation instead of being guessed at.

use anyhow::{bail, Result};

/// The boolean expression of a condition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expression {
    Literal(bool),
    Success,
    Failure,
    Always,
    Cancelled,
    Not(Box<Expression>),
    And(Box<Expression>, Box<Expression>),
    Or(Box<Expression>, Box<Expression>),
}

/// A parsed `if:` condition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Condition {
    /// Evaluated against the job's status, as GitHub does.
    Expression { expression: Expression, uses_status: bool },
    /// The condition reads a secret or a signing switch, which never has a
    /// value here. It is false, and the replay records that as a limit.
    Secret,
}

impl Condition {
    /// Read a step's `if:`. `None` or an empty condition is `success()`.
    pub fn parse(text: Option<&str>) -> Result<Condition> {
        let Some(text) = text.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(Condition::Expression { expression: Expression::Success, uses_status: true });
        };
        if text.contains("secrets.") || text.contains("SIGNING_CONFIGURED") || text.contains("github.token") {
            return Ok(Condition::Secret);
        }
        let inner = match text.strip_prefix("${{").and_then(|rest| rest.strip_suffix("}}")) {
            Some(inner) => inner.trim(),
            None => text,
        };
        let tokens = tokenize(inner)?;
        let mut parser = Parser { tokens: &tokens, position: 0 };
        let expression = parser.or()?;
        if parser.position != tokens.len() {
            bail!("Unsupported workflow condition: {text}");
        }
        let uses_status = expression.uses_status();
        Ok(Condition::Expression { expression, uses_status })
    }

    /// Whether the step runs, given whether an earlier step of the job failed.
    ///
    /// A condition without a status function is implicitly `success() && (…)`,
    /// so a step after a failure runs only when its condition says so.
    pub fn runs(&self, job_failed: bool) -> bool {
        match self {
            Condition::Secret => false,
            Condition::Expression { expression, uses_status } => {
                let value = expression.evaluate(job_failed);
                if *uses_status {
                    value
                } else {
                    !job_failed && value
                }
            }
        }
    }
}

impl Expression {
    fn uses_status(&self) -> bool {
        match self {
            Expression::Literal(_) => false,
            Expression::Success | Expression::Failure | Expression::Always | Expression::Cancelled => true,
            Expression::Not(inner) => inner.uses_status(),
            Expression::And(left, right) | Expression::Or(left, right) => left.uses_status() || right.uses_status(),
        }
    }

    /// A local replay is never cancelled: a timed-out step is a failed step.
    fn evaluate(&self, job_failed: bool) -> bool {
        match self {
            Expression::Literal(value) => *value,
            Expression::Success => !job_failed,
            Expression::Failure => job_failed,
            Expression::Always => true,
            Expression::Cancelled => false,
            Expression::Not(inner) => !inner.evaluate(job_failed),
            Expression::And(left, right) => left.evaluate(job_failed) && right.evaluate(job_failed),
            Expression::Or(left, right) => left.evaluate(job_failed) || right.evaluate(job_failed),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Word(String),
    Not,
    And,
    Or,
    Open,
    Close,
}

fn tokenize(text: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let characters: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        match character {
            ' ' | '\t' | '\n' | '\r' => index += 1,
            '!' if characters.get(index + 1) != Some(&'=') => {
                tokens.push(Token::Not);
                index += 1;
            }
            '&' if characters.get(index + 1) == Some(&'&') => {
                tokens.push(Token::And);
                index += 2;
            }
            '|' if characters.get(index + 1) == Some(&'|') => {
                tokens.push(Token::Or);
                index += 2;
            }
            '(' => {
                tokens.push(Token::Open);
                index += 1;
            }
            ')' => {
                tokens.push(Token::Close);
                index += 1;
            }
            value if value.is_ascii_alphanumeric() || value == '_' || value == '\'' => {
                let start = index;
                if value == '\'' {
                    index += 1;
                    while index < characters.len() && characters[index] != '\'' {
                        index += 1;
                    }
                    index += 1;
                } else {
                    while index < characters.len() && (characters[index].is_ascii_alphanumeric() || characters[index] == '_') {
                        index += 1;
                    }
                }
                tokens.push(Token::Word(characters[start..index.min(characters.len())].iter().collect()));
            }
            _ => bail!("Unsupported workflow condition: {text}"),
        }
    }
    Ok(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    position: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn or(&mut self) -> Result<Expression> {
        let mut left = self.and()?;
        while self.peek() == Some(&Token::Or) {
            self.position += 1;
            left = Expression::Or(Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Expression> {
        let mut left = self.unary()?;
        while self.peek() == Some(&Token::And) {
            self.position += 1;
            left = Expression::And(Box::new(left), Box::new(self.unary()?));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expression> {
        match self.peek() {
            Some(Token::Not) => {
                self.position += 1;
                Ok(Expression::Not(Box::new(self.unary()?)))
            }
            Some(Token::Open) => {
                self.position += 1;
                let inner = self.or()?;
                if self.peek() != Some(&Token::Close) {
                    bail!("Unsupported workflow condition: unbalanced parentheses");
                }
                self.position += 1;
                Ok(inner)
            }
            Some(Token::Word(word)) => {
                let word = word.clone();
                self.position += 1;
                let call = self.peek() == Some(&Token::Open)
                    && self.tokens.get(self.position + 1) == Some(&Token::Close);
                if call {
                    self.position += 2;
                    return Ok(match word.as_str() {
                        "success" => Expression::Success,
                        "failure" => Expression::Failure,
                        "always" => Expression::Always,
                        "cancelled" => Expression::Cancelled,
                        other => bail!("Unsupported workflow condition function: {other}()"),
                    });
                }
                Ok(match word.as_str() {
                    "true" | "'true'" | "1" => Expression::Literal(true),
                    "false" | "'false'" | "0" => Expression::Literal(false),
                    other => bail!("Unsupported workflow condition value: {other}"),
                })
            }
            _ => bail!("Unsupported workflow condition: missing operand"),
        }
    }
}
