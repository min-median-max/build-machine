//! `${{ }}` expressions, a step's `if:` and the templates of its other fields,
//! read as GitHub reads them.
//!
//! Only what is knowable on this machine is accepted: the job status functions,
//! literals, `!`, `&&`, `||`, `==`, `!=`, parentheses, the outputs of earlier
//! steps (`steps.<id>.outputs.<name>`), the replay's `github.event_name`,
//! `github.sha`, `github.ref` and `github.ref_name`, and `github.event.<path>`
//! of the event payload the replay is given. A step output has its
//! value once that step has run, so validation checks the reference and the
//! run supplies the value. Any other context — `github.actor`, `runner.temp`,
//! `hashFiles()` — has no local value, so an expression that reads one fails
//! validation instead of being guessed at.

use anyhow::{bail, Result};
use std::collections::BTreeMap;

/// The outputs each step with an `id` wrote to `$GITHUB_OUTPUT`, by step id.
pub type Outputs = BTreeMap<String, BTreeMap<String, String>>;

/// Where a job stands when a step's condition is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Success,
    /// An earlier step failed or exceeded its own `timeout-minutes`.
    Failure,
    /// The job exceeded its `timeout-minutes`, and GitHub cancelled it.
    Cancelled,
}

/// The result of a job, as `needs.<job>.result` reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobResult {
    Success,
    Failure,
    Cancelled,
    Skipped,
}

impl JobResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobResult::Success => "success",
            JobResult::Failure => "failure",
            JobResult::Cancelled => "cancelled",
            JobResult::Skipped => "skipped",
        }
    }
}

/// What a condition's status functions read: a step reads its job's status;
/// a job reads the results of the jobs it needs, directly or through them.
#[derive(Clone, Copy, Debug)]
enum Status<'a> {
    Step(JobStatus),
    Job(&'a [JobResult]),
}

/// The values an expression is evaluated against.
struct Scope<'a> {
    status: Status<'a>,
    outputs: &'a Outputs,
    github: &'a Github,
    needs: &'a BTreeMap<String, JobResult>,
}

/// The `github` values a replay has, as the event of that ref carries them
/// on GitHub.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Github {
    /// The payload of the replayed event, as GitHub sends it, which
    /// `github.event.<path>` reads. `None` when the replay was given none.
    pub event: Option<serde_json::Value>,
    /// The replay's event.
    pub event_name: String,
    /// The replayed revision.
    pub sha: String,
    /// `refs/heads/<branch>` or `refs/tags/<tag>`. `None` is a commit that
    /// no branch or tag names, and validation refuses a workflow that reads
    /// `github.ref` or `github.ref_name` then.
    pub reference: Option<String>,
}

/// The short name of `refs/heads/<branch>` or `refs/tags/<tag>`.
pub fn ref_name(reference: &str) -> &str {
    reference.strip_prefix("refs/heads/").or_else(|| reference.strip_prefix("refs/tags/")).unwrap_or(reference)
}

/// A `github` value an expression reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GithubValue {
    EventName,
    Sha,
    Ref,
    RefName,
    /// `github.event.<path>`, by the names of the path.
    Event(Vec<String>),
}

/// The value at `path` of an event payload, or `None` when the payload does
/// not hold it.
pub fn event_value<'a>(payload: &'a serde_json::Value, path: &[String]) -> Option<&'a serde_json::Value> {
    path.iter().try_fold(payload, |value, name| match value {
        serde_json::Value::Object(map) => map.get(name),
        serde_json::Value::Array(items) => name.parse::<usize>().ok().and_then(|index| items.get(index)),
        _ => None,
    })
}

/// A step output an expression reads.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reference {
    pub step: String,
    pub output: String,
}

/// An expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Expression {
    Literal(Value),
    Output(Reference),
    Github(GithubValue),
    /// `needs.<job>.result`, read in a job's `if`.
    NeedsResult(String),
    Success,
    Failure,
    Always,
    Cancelled,
    Not(Box<Expression>),
    And(Box<Expression>, Box<Expression>),
    Or(Box<Expression>, Box<Expression>),
    Equal(Box<Expression>, Box<Expression>),
    NotEqual(Box<Expression>, Box<Expression>),
}

/// A value, with GitHub's types.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
}

impl Value {
    /// GitHub's falsy values are `false`, `0`, `''` and `null`.
    fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(value) => *value,
            Value::Number(value) => *value != 0.0 && !value.is_nan(),
            Value::String(value) => !value.is_empty(),
        }
    }

    fn number(&self) -> f64 {
        match self {
            Value::Null => 0.0,
            Value::Bool(value) => f64::from(u8::from(*value)),
            Value::Number(value) => *value,
            Value::String(value) if value.trim().is_empty() => 0.0,
            Value::String(value) => value.trim().parse().unwrap_or(f64::NAN),
        }
    }

    /// GitHub compares strings without regard to case, and values of
    /// different types as numbers.
    fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::String(left), Value::String(right)) => left.to_lowercase() == right.to_lowercase(),
            (Value::Null, Value::Null) => true,
            (Value::Bool(left), Value::Bool(right)) => left == right,
            _ => self.number() == other.number(),
        }
    }

    /// How a value appears when it is placed in text.
    fn text(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => value.to_string(),
            Value::String(value) => value.clone(),
        }
    }
}

impl Expression {
    fn uses_status(&self) -> bool {
        match self {
            Expression::Literal(_) | Expression::Output(_) | Expression::Github(_) | Expression::NeedsResult(_) => false,
            Expression::Success | Expression::Failure | Expression::Always | Expression::Cancelled => true,
            Expression::Not(inner) => inner.uses_status(),
            Expression::And(left, right)
            | Expression::Or(left, right)
            | Expression::Equal(left, right)
            | Expression::NotEqual(left, right) => left.uses_status() || right.uses_status(),
        }
    }

    /// The `github.event` paths the expression reads, written with dots.
    fn event_paths(&self, found: &mut Vec<String>) {
        match self {
            Expression::Github(GithubValue::Event(path)) => found.push(path.join(".")),
            Expression::Not(inner) => inner.event_paths(found),
            Expression::And(left, right)
            | Expression::Or(left, right)
            | Expression::Equal(left, right)
            | Expression::NotEqual(left, right) => {
                left.event_paths(found);
                right.event_paths(found);
            }
            _ => {}
        }
    }

    /// Whether the expression reads `github.ref` or `github.ref_name`.
    fn reads_ref(&self) -> bool {
        match self {
            Expression::Github(value) => matches!(value, GithubValue::Ref | GithubValue::RefName),
            Expression::Not(inner) => inner.reads_ref(),
            Expression::And(left, right)
            | Expression::Or(left, right)
            | Expression::Equal(left, right)
            | Expression::NotEqual(left, right) => left.reads_ref() || right.reads_ref(),
            _ => false,
        }
    }

    fn needs(&self, found: &mut Vec<String>) {
        match self {
            Expression::NeedsResult(job) => found.push(job.clone()),
            Expression::Not(inner) => inner.needs(found),
            Expression::And(left, right)
            | Expression::Or(left, right)
            | Expression::Equal(left, right)
            | Expression::NotEqual(left, right) => {
                left.needs(found);
                right.needs(found);
            }
            _ => {}
        }
    }

    fn references(&self, found: &mut Vec<Reference>) {
        match self {
            Expression::Output(reference) => found.push(reference.clone()),
            Expression::Not(inner) => inner.references(found),
            Expression::And(left, right)
            | Expression::Or(left, right)
            | Expression::Equal(left, right)
            | Expression::NotEqual(left, right) => {
                left.references(found);
                right.references(found);
            }
            _ => {}
        }
    }

    /// `&&` and `||` yield an operand, as GitHub's do, so
    /// `steps.a.outputs.v || 'none'` is a value and not a boolean.
    fn evaluate(&self, status: JobStatus, outputs: &Outputs, github: &Github) -> Value {
        let needs = BTreeMap::new();
        self.evaluate_in(&Scope { status: Status::Step(status), outputs, github, needs: &needs })
    }

    fn evaluate_in(&self, scope: &Scope) -> Value {
        let github = scope.github;
        match self {
            Expression::Literal(value) => value.clone(),
            Expression::Output(reference) => Value::String(
                scope
                    .outputs
                    .get(&reference.step)
                    .and_then(|values| values.get(&reference.output))
                    .cloned()
                    .unwrap_or_default(),
            ),
            Expression::Github(value) => match value {
                GithubValue::EventName => Value::String(github.event_name.clone()),
                GithubValue::Sha => Value::String(github.sha.clone()),
                GithubValue::Ref => github.reference.clone().map_or(Value::Null, Value::String),
                GithubValue::RefName => github.reference.as_deref().map_or(Value::Null, |reference| Value::String(ref_name(reference).to_owned())),
                GithubValue::Event(path) => {
                    match github.event.as_ref().and_then(|payload| event_value(payload, path)) {
                        None | Some(serde_json::Value::Null) => Value::Null,
                        Some(serde_json::Value::Bool(value)) => Value::Bool(*value),
                        Some(serde_json::Value::Number(value)) => value.as_f64().map_or(Value::Null, Value::Number),
                        Some(serde_json::Value::String(value)) => Value::String(value.clone()),
                        // An object or an array is placed as JSON, as GitHub places it.
                        Some(other) => Value::String(other.to_string()),
                    }
                }
            },
            Expression::NeedsResult(job) => {
                scope.needs.get(job).map_or(Value::Null, |result| Value::String(result.as_str().to_owned()))
            }
            // A job's success() is every job before it succeeding; failure()
            // is one of them failing or being cancelled. A skipped job is
            // neither. A local replay is never cancelled as a whole.
            Expression::Success => Value::Bool(match scope.status {
                Status::Step(status) => status == JobStatus::Success,
                Status::Job(results) => results.iter().all(|result| *result == JobResult::Success),
            }),
            Expression::Failure => Value::Bool(match scope.status {
                Status::Step(status) => status == JobStatus::Failure,
                Status::Job(results) => {
                    results.iter().any(|result| matches!(result, JobResult::Failure | JobResult::Cancelled))
                }
            }),
            Expression::Always => Value::Bool(true),
            Expression::Cancelled => Value::Bool(match scope.status {
                Status::Step(status) => status == JobStatus::Cancelled,
                Status::Job(_) => false,
            }),
            Expression::Not(inner) => Value::Bool(!inner.evaluate_in(scope).truthy()),
            Expression::And(left, right) => {
                let left = left.evaluate_in(scope);
                if left.truthy() {
                    right.evaluate_in(scope)
                } else {
                    left
                }
            }
            Expression::Or(left, right) => {
                let left = left.evaluate_in(scope);
                if left.truthy() {
                    left
                } else {
                    right.evaluate_in(scope)
                }
            }
            Expression::Equal(left, right) => Value::Bool(left.evaluate_in(scope).equals(&right.evaluate_in(scope))),
            Expression::NotEqual(left, right) => {
                Value::Bool(!left.evaluate_in(scope).equals(&right.evaluate_in(scope)))
            }
        }
    }

    /// Read one expression, the text inside `${{ }}`.
    pub fn parse(text: &str) -> Result<Expression> {
        let tokens = tokenize(text)?;
        let mut parser = Parser { tokens: &tokens, position: 0 };
        let expression = parser.or()?;
        if parser.position != tokens.len() {
            bail!("Unsupported workflow expression: {text}");
        }
        Ok(expression)
    }
}

/// A parsed `if:` condition.
#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    /// Evaluated against the job's status and the step outputs, as GitHub does.
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
        let expression = Expression::parse(inner)?;
        let uses_status = expression.uses_status();
        Ok(Condition::Expression { expression, uses_status })
    }

    /// Whether the step runs, given where its job stands and what earlier
    /// steps wrote.
    ///
    /// A condition without a status function is implicitly `success() && (…)`,
    /// so a step after a failure or a cancellation runs only when its
    /// condition says so.
    pub fn runs(&self, status: JobStatus, outputs: &Outputs, github: &Github) -> bool {
        match self {
            Condition::Secret => false,
            Condition::Expression { expression, uses_status } => {
                let value = expression.evaluate(status, outputs, github).truthy();
                if *uses_status {
                    value
                } else {
                    status == JobStatus::Success && value
                }
            }
        }
    }

    /// Whether a job with this `if` runs, given the results of the jobs before
    /// it (`ancestors`, the jobs it needs and theirs) and of the jobs it needs
    /// directly. Without a status function the condition is
    /// `success() && (…)`, GitHub's default for a job.
    pub fn job_runs(&self, ancestors: &[JobResult], needs: &BTreeMap<String, JobResult>, github: &Github) -> bool {
        match self {
            Condition::Secret => false,
            Condition::Expression { expression, uses_status } => {
                let outputs = Outputs::new();
                let scope = Scope { status: Status::Job(ancestors), outputs: &outputs, github, needs };
                let value = expression.evaluate_in(&scope).truthy();
                let succeeded = ancestors.iter().all(|result| *result == JobResult::Success);
                if *uses_status {
                    value
                } else {
                    succeeded && value
                }
            }
        }
    }

    /// The jobs whose `needs.<job>.result` the condition reads.
    pub fn needs(&self) -> Vec<String> {
        let mut found = Vec::new();
        if let Condition::Expression { expression, .. } = self {
            expression.needs(&mut found);
        }
        found
    }

    /// Whether the condition reads `github.ref` or `github.ref_name`.
    pub fn reads_ref(&self) -> bool {
        matches!(self, Condition::Expression { expression, .. } if expression.reads_ref())
    }

    /// The `github.event` paths the condition reads.
    pub fn event_paths(&self) -> Vec<String> {
        let mut found = Vec::new();
        if let Condition::Expression { expression, .. } = self {
            expression.event_paths(&mut found);
        }
        found
    }

    /// The step outputs the condition reads.
    pub fn references(&self) -> Vec<Reference> {
        let mut found = Vec::new();
        if let Condition::Expression { expression, .. } = self {
            expression.references(&mut found);
        }
        found
    }
}

/// Text with `${{ }}` expressions in it: a `run:` block, an `env` or `with`
/// value, a `working-directory`.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    parts: Vec<Part>,
}

#[derive(Clone, Debug, PartialEq)]
enum Part {
    Text(String),
    Expression(Expression),
}

impl Template {
    pub fn parse(text: &str) -> Result<Template> {
        let mut parts = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find("${{") {
            if start > 0 {
                parts.push(Part::Text(rest[..start].to_owned()));
            }
            let after = &rest[start + 3..];
            let Some(end) = after.find("}}") else {
                bail!("Unterminated workflow expression: {text}");
            };
            let expression = Expression::parse(after[..end].trim())?;
            let mut needs = Vec::new();
            expression.needs(&mut needs);
            if let Some(job) = needs.first() {
                bail!("needs.{job}.result is only read in a job's if: {text}");
            }
            if expression.uses_status() {
                bail!("A status function is only available in if: {text}");
            }
            parts.push(Part::Expression(expression));
            rest = &after[end + 2..];
        }
        if !rest.is_empty() {
            parts.push(Part::Text(rest.to_owned()));
        }
        Ok(Template { parts })
    }

    pub fn render(&self, outputs: &Outputs, github: &Github) -> String {
        self.parts
            .iter()
            .map(|part| match part {
                Part::Text(text) => text.clone(),
                Part::Expression(expression) => expression.evaluate(JobStatus::Success, outputs, github).text(),
            })
            .collect()
    }

    /// Whether the template reads `github.ref` or `github.ref_name`.
    pub fn reads_ref(&self) -> bool {
        self.parts.iter().any(|part| matches!(part, Part::Expression(expression) if expression.reads_ref()))
    }

    /// The `github.event` paths the template reads.
    pub fn event_paths(&self) -> Vec<String> {
        let mut found = Vec::new();
        for part in &self.parts {
            if let Part::Expression(expression) = part {
                expression.event_paths(&mut found);
            }
        }
        found
    }

    pub fn references(&self) -> Vec<Reference> {
        let mut found = Vec::new();
        for part in &self.parts {
            if let Part::Expression(expression) = part {
                expression.references(&mut found);
            }
        }
        found
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    Text(String),
    Number(f64),
    Not,
    And,
    Or,
    Equal,
    NotEqual,
    Open,
    Close,
}

fn tokenize(text: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let characters: Vec<char> = text.chars().collect();
    let mut index = 0;
    let pair = |index: usize, second: char| characters.get(index + 1) == Some(&second);
    while index < characters.len() {
        let character = characters[index];
        match character {
            ' ' | '\t' | '\n' | '\r' => index += 1,
            '!' if pair(index, '=') => {
                tokens.push(Token::NotEqual);
                index += 2;
            }
            '!' => {
                tokens.push(Token::Not);
                index += 1;
            }
            '=' if pair(index, '=') => {
                tokens.push(Token::Equal);
                index += 2;
            }
            '&' if pair(index, '&') => {
                tokens.push(Token::And);
                index += 2;
            }
            '|' if pair(index, '|') => {
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
            '\'' => {
                // A quote inside a string is written twice.
                let mut value = String::new();
                index += 1;
                loop {
                    match characters.get(index) {
                        None => bail!("Unterminated string in workflow expression: {text}"),
                        Some('\'') if characters.get(index + 1) == Some(&'\'') => {
                            value.push('\'');
                            index += 2;
                        }
                        Some('\'') => {
                            index += 1;
                            break;
                        }
                        Some(other) => {
                            value.push(*other);
                            index += 1;
                        }
                    }
                }
                tokens.push(Token::Text(value));
            }
            value if value.is_ascii_digit() => {
                let start = index;
                while index < characters.len() && (characters[index].is_ascii_digit() || characters[index] == '.') {
                    index += 1;
                }
                let number: String = characters[start..index].iter().collect();
                let Ok(number) = number.parse() else { bail!("Unsupported number in workflow expression: {text}") };
                tokens.push(Token::Number(number));
            }
            value if value.is_ascii_alphabetic() || value == '_' => {
                let start = index;
                while index < characters.len()
                    && (characters[index].is_ascii_alphanumeric() || matches!(characters[index], '_' | '-' | '.'))
                {
                    index += 1;
                }
                tokens.push(Token::Word(characters[start..index].iter().collect()));
            }
            _ => bail!("Unsupported workflow expression: {text}"),
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
        let mut left = self.comparison()?;
        while self.peek() == Some(&Token::And) {
            self.position += 1;
            left = Expression::And(Box::new(left), Box::new(self.comparison()?));
        }
        Ok(left)
    }

    fn comparison(&mut self) -> Result<Expression> {
        let mut left = self.unary()?;
        loop {
            match self.peek() {
                Some(Token::Equal) => {
                    self.position += 1;
                    left = Expression::Equal(Box::new(left), Box::new(self.unary()?));
                }
                Some(Token::NotEqual) => {
                    self.position += 1;
                    left = Expression::NotEqual(Box::new(left), Box::new(self.unary()?));
                }
                _ => return Ok(left),
            }
        }
    }

    fn unary(&mut self) -> Result<Expression> {
        match self.peek().cloned() {
            Some(Token::Not) => {
                self.position += 1;
                Ok(Expression::Not(Box::new(self.unary()?)))
            }
            Some(Token::Open) => {
                self.position += 1;
                let inner = self.or()?;
                if self.peek() != Some(&Token::Close) {
                    bail!("Unsupported workflow expression: unbalanced parentheses");
                }
                self.position += 1;
                Ok(inner)
            }
            Some(Token::Text(value)) => {
                self.position += 1;
                Ok(Expression::Literal(Value::String(value)))
            }
            Some(Token::Number(value)) => {
                self.position += 1;
                Ok(Expression::Literal(Value::Number(value)))
            }
            Some(Token::Word(word)) => {
                self.position += 1;
                let call = self.peek() == Some(&Token::Open) && self.tokens.get(self.position + 1) == Some(&Token::Close);
                if call {
                    self.position += 2;
                    return Ok(match word.as_str() {
                        "success" => Expression::Success,
                        "failure" => Expression::Failure,
                        "always" => Expression::Always,
                        "cancelled" => Expression::Cancelled,
                        other => bail!("Unsupported workflow expression function: {other}()"),
                    });
                }
                if self.peek() == Some(&Token::Open) {
                    bail!("Unsupported workflow expression function: {word}()");
                }
                Ok(match word.as_str() {
                    "true" => Expression::Literal(Value::Bool(true)),
                    "false" => Expression::Literal(Value::Bool(false)),
                    "null" => Expression::Literal(Value::Null),
                    path => context(path)?,
                })
            }
            _ => bail!("Unsupported workflow expression: missing operand"),
        }
    }
}

/// `steps.<id>.outputs.<name>`, four `github` values and `github.event.<path>`
/// are the contexts with a local value.
fn context(path: &str) -> Result<Expression> {
    let parts: Vec<&str> = path.split('.').collect();
    Ok(match parts.as_slice() {
        ["steps", step, "outputs", output] if !step.is_empty() && !output.is_empty() => {
            Expression::Output(Reference { step: (*step).to_owned(), output: (*output).to_owned() })
        }
        ["github", "event_name"] => Expression::Github(GithubValue::EventName),
        ["github", "sha"] => Expression::Github(GithubValue::Sha),
        ["github", "ref"] => Expression::Github(GithubValue::Ref),
        ["github", "ref_name"] => Expression::Github(GithubValue::RefName),
        ["github", "event", path @ ..] if !path.is_empty() && path.iter().all(|name| !name.is_empty()) => {
            Expression::Github(GithubValue::Event(path.iter().map(|name| (*name).to_owned()).collect()))
        }
        ["needs", job, "result"] if !job.is_empty() => Expression::NeedsResult((*job).to_owned()),
        _ => bail!(
            "Unsupported workflow context: {path}. Only steps.<id>.outputs.<name>, github.event_name, github.sha, github.ref, github.ref_name and github.event.<path> have a value in a local replay."
        ),
    })
}
