//! A Mustache engine that reproduces the jmustache behaviour openapi-generator relies on:
//! standalone-line trimming, `-first`/`-last`/`-index`, numeric list indices (`list.0`),
//! compound names, set-delimiter tags, `defaultValue("")` for missing variables,
//! HTML escaping, and the `lambda.*` sections.

use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone)]
enum Seg {
    Text(Text),
    Var {
        name: String,
        escape: bool,
    },
    Block {
        name: String,
        inverted: bool,
        segs: Vec<Seg>,
    },
    Partial {
        name: String,
    },
    /// Comments and delimiter changes: render nothing but take part in trimming.
    Faux,
}

/// A text segment with jmustache's precomputed blank positions.
/// `lead`: index of the first '\n' if only whitespace precedes it.
/// `trail`: index just after the last '\n' if only whitespace follows it.
#[derive(Debug, Clone)]
struct Text {
    s: String,
    lead: Option<usize>,
    trail: Option<usize>,
}

impl Text {
    fn new(s: String, first: bool) -> Self {
        let lead = blank_pos(&s, true, first);
        let trail = blank_pos(&s, false, first);
        Text { s, lead, trail }
    }
    fn trim_lead(&self) -> Text {
        match self.lead {
            None => self.clone(),
            Some(l) => {
                let cut = l + 1;
                Text {
                    s: self.s[cut.min(self.s.len())..].to_string(),
                    lead: None,
                    trail: self.trail.map(|t| t.saturating_sub(cut)),
                }
            }
        }
    }
    fn trim_trail(&self) -> Text {
        match self.trail {
            None => self.clone(),
            Some(t) => Text {
                s: self.s[..t].to_string(),
                lead: self.lead,
                trail: None,
            },
        }
    }
}

fn blank_pos(s: &str, leading: bool, first: bool) -> Option<usize> {
    let b = s.as_bytes();
    if leading {
        for (i, &c) in b.iter().enumerate() {
            if c == b'\n' {
                return Some(i);
            }
            if !(c as char).is_whitespace() {
                return None;
            }
        }
        None
    } else {
        for i in (0..b.len()).rev() {
            let c = b[i];
            if c == b'\n' {
                return Some(i + 1);
            }
            if !(c as char).is_whitespace() {
                return None;
            }
        }
        if first { Some(0) } else { None }
    }
}

#[derive(Debug)]
pub struct Template {
    segs: Vec<Seg>,
}

pub fn compile(src: &str) -> Result<Template, String> {
    let mut otag = "{{".to_string();
    let mut ctag = "}}".to_string();
    // stack of (name, inverted, segs)
    let mut stack: Vec<(String, bool, Vec<Seg>)> = vec![(String::new(), false, Vec::new())];
    let mut pos = 0;
    let bytes = src;
    while pos < bytes.len() {
        let Some(rel) = bytes[pos..].find(&otag) else {
            push_text(&mut stack, &bytes[pos..], pos == 0);
            break;
        };
        let start = pos + rel;
        if start > pos {
            push_text(&mut stack, &bytes[pos..start], pos == 0);
        }
        let after = start + otag.len();
        // triple mustache
        if otag == "{{" && bytes[after..].starts_with('{') {
            let end = bytes[after..].find("}}}").ok_or("unclosed {{{")? + after;
            let name = bytes[after + 1..end].trim().to_string();
            stack.last_mut().unwrap().2.push(Seg::Var {
                name,
                escape: false,
            });
            pos = end + 3;
            continue;
        }
        let end = bytes[after..]
            .find(&ctag)
            .ok_or_else(|| format!("unclosed tag at {start}"))?
            + after;
        let inner = &bytes[after..end];
        // jmustache: a new opening delimiter inside a tag ("{{\n{{/x}}") turns the
        // text matched so far into plain text and starts a new tag there.
        let open_char = otag.chars().next().unwrap();
        if !inner.starts_with('!')
            && let Some(k) = inner.find(open_char).filter(|&k| k > 0)
        {
            push_text(&mut stack, &bytes[start..after + k], false);
            pos = after + k;
            continue;
        }
        pos = end + ctag.len();
        let (kind, rest) = match inner.chars().next() {
            Some(c @ ('#' | '^' | '/' | '>' | '!' | '&' | '=')) => (Some(c), &inner[1..]),
            _ => (None, inner),
        };
        let name = rest.trim().to_string();
        let segs = &mut stack.last_mut().unwrap().2;
        match kind {
            None => segs.push(Seg::Var { name, escape: true }),
            Some('&') => segs.push(Seg::Var {
                name,
                escape: false,
            }),
            Some('!') => segs.push(Seg::Faux),
            Some('>') => segs.push(Seg::Partial { name }),
            Some('=') => {
                let spec = rest.trim_end_matches('=').trim();
                let mut it = spec.split_whitespace();
                otag = it.next().ok_or("bad delimiter")?.to_string();
                ctag = it.next().ok_or("bad delimiter")?.to_string();
                segs.push(Seg::Faux);
            }
            Some(c @ ('#' | '^')) => stack.push((name, c == '^', Vec::new())),
            Some('/') => {
                let (open, inverted, mut inner_segs) = stack.pop().ok_or("unbalanced close")?;
                if open != name || stack.is_empty() {
                    return Err(format!("mismatched close tag {name} (open {open})"));
                }
                trim(&mut inner_segs, false);
                stack.last_mut().unwrap().2.push(Seg::Block {
                    name,
                    inverted,
                    segs: inner_segs,
                });
            }
            _ => unreachable!(),
        }
    }
    if stack.len() != 1 {
        return Err(format!("unclosed section {}", stack.last().unwrap().0));
    }
    let mut segs = stack.pop().unwrap().2;
    trim(&mut segs, true);
    Ok(Template { segs })
}

fn push_text(stack: &mut [(String, bool, Vec<Seg>)], s: &str, first: bool) {
    stack
        .last_mut()
        .unwrap()
        .2
        .push(Seg::Text(Text::new(s.to_string(), first)));
}

fn first_leads_blank(segs: &[Seg]) -> bool {
    matches!(segs.first(), Some(Seg::Text(t)) if t.lead.is_some())
}
fn last_trails_blank(segs: &[Seg]) -> bool {
    matches!(segs.last(), Some(Seg::Text(t)) if t.trail.is_some())
}

/// jmustache `Mustache.trim`: strip whitespace around tags that stand alone on their line.
fn trim(segs: &mut [Seg], top: bool) {
    let n = segs.len();
    for i in 0..n {
        let prev_blank = if i == 0 {
            top
        } else {
            matches!(&segs[i - 1], Seg::Text(t) if t.trail.is_some())
        };
        let next_blank = if i + 1 == n {
            top
        } else {
            matches!(&segs[i + 1], Seg::Text(t) if t.lead.is_some())
        };
        match &mut segs[i] {
            Seg::Block { segs: inner, .. } => {
                let mut trim_prev = false;
                let mut trim_next = false;
                if prev_blank && first_leads_blank(inner) {
                    trim_prev = true;
                    if let Some(Seg::Text(t)) = inner.first_mut() {
                        *t = t.trim_lead();
                    }
                }
                if next_blank && last_trails_blank(inner) {
                    trim_next = true;
                    if let Some(Seg::Text(t)) = inner.last_mut() {
                        *t = t.trim_trail();
                    }
                }
                if trim_prev
                    && i > 0
                    && let Seg::Text(t) = &mut segs[i - 1]
                {
                    *t = t.trim_trail();
                }
                if trim_next
                    && i + 1 < n
                    && let Seg::Text(t) = &mut segs[i + 1]
                {
                    *t = t.trim_lead();
                }
            }
            // jmustache (as used by openapi-generator 7.8) does not trim standalone partial lines.
            Seg::Faux if prev_blank && next_blank => {
                if i > 0
                    && let Seg::Text(t) = &mut segs[i - 1]
                {
                    *t = t.trim_trail();
                }
                if i + 1 < n
                    && let Seg::Text(t) = &mut segs[i + 1]
                {
                    *t = t.trim_lead();
                }
            }
            _ => {}
        }
    }
}

pub type Lambda = fn(&str) -> String;

pub struct Engine {
    templates: HashMap<String, Template>,
    lambdas: HashMap<&'static str, Lambda>,
}

struct Frame<'a> {
    value: &'a Value,
    first: bool,
    last: bool,
    index: usize,
}

impl Engine {
    pub fn new(lambdas: HashMap<&'static str, Lambda>) -> Self {
        Engine {
            templates: HashMap::new(),
            lambdas,
        }
    }

    pub fn add(&mut self, name: &str, src: &str) -> Result<(), String> {
        let t = compile(src).map_err(|e| format!("{name}: {e}"))?;
        self.templates.insert(name.to_string(), t);
        Ok(())
    }

    pub fn render(&self, name: &str, ctx: &Value) -> Result<String, String> {
        let t = self
            .templates
            .get(name)
            .ok_or_else(|| format!("no template {name}"))?;
        let mut out = String::with_capacity(4096);
        let mut stack = vec![Frame {
            value: ctx,
            first: false,
            last: false,
            index: 0,
        }];
        self.exec(&t.segs, &mut stack, &mut out)?;
        Ok(out)
    }

    fn exec<'v>(
        &self,
        segs: &[Seg],
        stack: &mut Vec<Frame<'v>>,
        out: &mut String,
    ) -> Result<(), String> {
        for seg in segs {
            match seg {
                Seg::Text(t) => out.push_str(&t.s),
                Seg::Faux => {}
                Seg::Var { name, escape } => {
                    let s = match lookup(stack, name) {
                        Lookup::Val(v) => to_str(v),
                        Lookup::Bool(b) => b.to_string(),
                        Lookup::Num(n) => n.to_string(),
                        Lookup::Missing => String::new(),
                    };
                    if *escape {
                        html_escape(&s, out)
                    } else {
                        out.push_str(&s)
                    }
                }
                Seg::Partial { name } => {
                    let t = self
                        .templates
                        .get(name)
                        .ok_or_else(|| format!("no partial {name}"))?;
                    self.exec(&t.segs, stack, out)?;
                }
                Seg::Block {
                    name,
                    inverted,
                    segs: inner,
                } => {
                    if let Some(lam) = name.strip_prefix("lambda.") {
                        if *inverted {
                            continue;
                        }
                        let f = self
                            .lambdas
                            .get(lam)
                            .ok_or_else(|| format!("unknown lambda {lam}"))?;
                        let mut buf = String::new();
                        self.exec(inner, stack, &mut buf)?;
                        out.push_str(&f(&buf));
                        continue;
                    }
                    let found = lookup(stack, name);
                    if *inverted {
                        if is_falsey(&found) {
                            self.exec(inner, stack, out)?;
                        }
                        continue;
                    }
                    match found {
                        Lookup::Missing => {}
                        Lookup::Bool(b) => {
                            if b {
                                self.exec(inner, stack, out)?
                            }
                        }
                        Lookup::Num(_) => self.exec(inner, stack, out)?,
                        Lookup::Val(v) => match v {
                            Value::Null => {}
                            Value::Bool(false) => {}
                            Value::Bool(true) => self.exec(inner, stack, out)?,
                            Value::Array(items) => {
                                let len = items.len();
                                for (i, item) in items.iter().enumerate() {
                                    stack.push(Frame {
                                        value: item,
                                        first: i == 0,
                                        last: i + 1 == len,
                                        index: i + 1,
                                    });
                                    self.exec(inner, stack, out)?;
                                    stack.pop();
                                }
                            }
                            other => {
                                let top = stack.last().unwrap();
                                let (first, last, index) = (top.first, top.last, top.index);
                                stack.push(Frame {
                                    value: other,
                                    first,
                                    last,
                                    index,
                                });
                                self.exec(inner, stack, out)?;
                                stack.pop();
                            }
                        },
                    }
                }
            }
        }
        Ok(())
    }
}

enum Lookup<'a> {
    Val(&'a Value),
    Bool(bool),
    Num(usize),
    Missing,
}

fn is_falsey(l: &Lookup) -> bool {
    match l {
        Lookup::Missing => true,
        Lookup::Bool(b) => !b,
        Lookup::Num(_) => false,
        Lookup::Val(v) => match v {
            Value::Null => true,
            Value::Bool(b) => !b,
            Value::Array(a) => a.is_empty(),
            _ => false,
        },
    }
}

fn lookup<'a>(stack: &[Frame<'a>], name: &str) -> Lookup<'a> {
    let top = stack.last().unwrap();
    match name {
        "." | "this" => return Lookup::Val(top.value),
        "-first" => return Lookup::Bool(top.first),
        "-last" => return Lookup::Bool(top.last),
        "-index" => return Lookup::Num(top.index),
        _ => {}
    }
    let mut parts = name.split('.');
    let head = parts.next().unwrap();
    let mut cur = None;
    for f in stack.iter().rev() {
        if let Some(v) = get(f.value, head) {
            cur = Some(v);
            break;
        }
    }
    let Some(mut cur) = cur else {
        return Lookup::Missing;
    };
    for p in parts {
        match get(cur, p) {
            Some(v) => cur = v,
            None => return Lookup::Missing,
        }
    }
    Lookup::Val(cur)
}

fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => m.get(key),
        Value::Array(a) => key.parse::<usize>().ok().and_then(|i| a.get(i)),
        _ => None,
    }
}

fn to_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

pub fn html_escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\'' => out.push_str("&#39;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '`' => out.push_str("&#x60;"),
            '=' => out.push_str("&#x3D;"),
            c => out.push(c),
        }
    }
}
