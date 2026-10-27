//! What the client checks in a manifest, and what a call or a write is held
//! to. The types live in the crate root; this module is the rules over them.

use crate::*;

/// A manifest the client refuses.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ManifestError {
    #[error("\"{0}\" isn't a valid name")]
    BadName(String),
    #[error("\"{0}\" is declared twice")]
    Duplicate(String),
    #[error("\"{0}\" has no type")]
    Untyped(String),
    #[error("\"{0}\" has an empty range or tuple, or exceeds the client's limits")]
    BadShape(String),
    #[error("{method} moves \"{field}\", which isn't a writable field")]
    MovesUnknown { method: String, field: String },
    #[error("\"{0}\" isn't an OSC prefix it may use")]
    BadPrefix(String),
    #[error("its terms are unusable: {0}")]
    BadTerms(String),
    #[error("action \"{0}\" calls no method it has, or it has no field to set")]
    BadAction(String),
    #[error("condition \"{0}\" names no on/off or number field it has")]
    BadCondition(String),
    #[error("it declares more than {SECRETS_MAX} secret slots")]
    TooManySecrets,
    #[error("it declares more than {OSC_PREFIXES_MAX} OSC prefixes to read or to write")]
    TooManyPrefixes,
    #[error("it declares more than {PAGES_MAX} pages")]
    TooManyPages,
    #[error("it declares more than {LOG_PREFIXES_MAX} log prefixes")]
    TooManyLogPrefixes,
    #[error("it declares more than {NET_GRANTS_MAX} network grants")]
    TooManyNetGrants,
    #[error(
        "network grant {0} names no address mask, mDNS name or domain it may use, or no ports"
    )]
    BadNetGrant(usize),
    #[error("\"{0}\" isn't a log prefix: some text of at most {LOG_PREFIX_MAX} bytes without control characters")]
    BadLogPrefix(String),
    #[error("@{0} declares more than {FIELDS_MAX} fields, {METHODS_MAX} methods, {SIGNALS_MAX} signals or {PARAMS_MAX} parameters to a method")]
    TooLarge(String),
    #[error("it packages more than {ICONS_MAX} icons")]
    TooManyIcons,
    #[error("icon \"{0}\" isn't a store key, repeats one, is over {ICON_MAX} bytes or reaches outside itself")]
    BadIcon(String),
}

/// A call or a write the client refuses.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CheckError {
    #[error("no field \"{0}\"")]
    UnknownField(String),
    #[error("\"{0}\" is read-only")]
    ReadOnly(String),
    #[error("no method \"{0}\"")]
    UnknownMethod(String),
    #[error("{method} takes {expected} arguments, not {got}")]
    Arity {
        method: String,
        expected: usize,
        got: usize,
    },
    #[error("\"{name}\" takes {expected:?}, not {got:?}")]
    Type {
        name: String,
        expected: Type,
        got: Type,
    },
    #[error("\"{0}\" takes a finite number")]
    NotFinite(String),
    #[error("\"{name}\" takes at most {max} bytes of text")]
    TooLong { name: String, max: usize },
    #[error("\"{name}\" takes at most {max} items")]
    TooMany { name: String, max: usize },
    #[error("\"{name}\" takes {expected} items, not {got}")]
    Length {
        name: String,
        expected: usize,
        got: usize,
    },
}

/// A Wex identifier: a letter or `_`, then letters, digits or `_`.
/// Whether `name` is an identifier: an ASCII letter or `_`, then ASCII
/// letters, digits and `_`.
pub fn is_ident(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A name people read: 1 to 64 bytes, no control characters, not blank.
/// A window's title is one.
pub fn is_label(name: &str) -> bool {
    is_text_label(name, 64)
}

/// Text people read: 1 to `max` bytes, no control characters, not blank.
pub(crate) fn is_text_label(text: &str, max: usize) -> bool {
    text.len() <= max && !text.trim().is_empty() && !text.chars().any(char::is_control)
}

/// The longest label a list item takes, in bytes.
pub const OPTION_LABEL_MAX: usize = 256;

/// A namespace name: an identifier in capitals (`CAMERA`).
fn is_namespace_name(name: &str) -> bool {
    is_ident(name) && !name.chars().any(|c| c.is_ascii_lowercase())
}

/// An OSC prefix: an address followed by `/`, never reaching avatar
/// addresses in any case.
fn is_prefix(prefix: &str) -> bool {
    prefix.strip_suffix('/').is_some_and(is_osc_address)
        && !prefix.to_ascii_lowercase().starts_with("/avatar/")
}

/// A literal OSC address of at most [`OSC_ADDR_MAX`] bytes: `/`-separated
/// parts, none empty, of printable ASCII without the characters OSC 1.0
/// reserves for patterns and type tags.
pub(crate) fn is_osc_address(addr: &str) -> bool {
    addr.len() <= OSC_ADDR_MAX
        && addr.strip_prefix('/').is_some_and(|parts| {
            parts.split('/').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_graphic() && !b"#*,?[]{}".contains(&b))
            })
        })
}

/// A field's or a parameter's shape: something, sound and within the
/// ceilings.
fn check_shape(name: &str, shape: &Shape) -> Result<(), ManifestError> {
    if *shape == Shape::None {
        return Err(ManifestError::Untyped(name.to_owned()));
    }
    if !shape.sound() || shape.depth() > DEPTH_MAX || shape.leaves() > VALUES_MAX {
        return Err(ManifestError::BadShape(name.to_owned()));
    }
    Ok(())
}

impl Manifest {
    pub fn validate(&self) -> Result<(), ManifestError> {
        if !is_ident(&self.name) {
            return Err(ManifestError::BadName(self.name.clone()));
        }
        if !is_label(&self.author) {
            return Err(ManifestError::BadName(self.author.clone()));
        }
        if let Some(ns) = &self.namespace {
            ns.validate()?;
        }
        if self.osc_read.len() > OSC_PREFIXES_MAX || self.osc_write.len() > OSC_PREFIXES_MAX {
            return Err(ManifestError::TooManyPrefixes);
        }
        if let Some(p) = self
            .osc_read
            .iter()
            .chain(&self.osc_write)
            .find(|p| !is_prefix(p))
        {
            return Err(ManifestError::BadPrefix(p.clone()));
        }
        if let Some(t) = &self.terms {
            let usable = is_label(&t.label)
                && (!t.inline.trim().is_empty() || !t.url.trim().is_empty())
                && t.inline.len() <= TERMS_INLINE_MAX
                && (t.url.trim().is_empty() || t.url.starts_with("https://"))
                && (t.sha256.is_empty()
                    || (t.sha256.len() == 64
                        && t.sha256
                            .chars()
                            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())));
            if !usable {
                return Err(ManifestError::BadTerms(t.label.clone()));
            }
        }
        let mut labels: Vec<&str> = Vec::new();
        for a in &self.actions {
            if !is_label(&a.label) {
                return Err(ManifestError::BadName(a.label.clone()));
            }
            if labels.contains(&a.label.as_str()) {
                return Err(ManifestError::Duplicate(a.label.clone()));
            }
            labels.push(&a.label);
            let ns = self.namespace.as_ref();
            let sound = match &a.does {
                Does::Call(m) => ns.is_some_and(|n| n.methods.iter().any(|x| &x.name == m)),
                Does::Set => ns.is_some_and(|n| n.fields.iter().any(|f| f.writable)),
            };
            if !sound {
                return Err(ManifestError::BadAction(a.label.clone()));
            }
        }
        let mut labels: Vec<&str> = self
            .namespace
            .iter()
            .flat_map(|n| &n.signals)
            .map(|e| e.label.as_str())
            .collect();
        for c in &self.conditions {
            if !is_label(&c.label) {
                return Err(ManifestError::BadName(c.label.clone()));
            }
            if labels.contains(&c.label.as_str()) {
                return Err(ManifestError::Duplicate(c.label.clone()));
            }
            labels.push(&c.label);
            let comparable = self
                .namespace
                .as_ref()
                .and_then(|n| n.fields.iter().find(|f| f.name == c.field))
                .is_some_and(|f| matches!(f.shape, Shape::Bool | Shape::Number(_)));
            if !comparable {
                return Err(ManifestError::BadCondition(c.label.clone()));
            }
        }
        let mut pages: Vec<&str> = Vec::new();
        if self.pages.len() > PAGES_MAX {
            return Err(ManifestError::TooManyPages);
        }
        for page in &self.pages {
            if !is_ident(&page.id) || page.id.len() > 64 {
                return Err(ManifestError::BadName(page.id.clone()));
            }
            if !is_label(&page.title) {
                return Err(ManifestError::BadName(page.title.clone()));
            }
            if pages.contains(&page.id.as_str()) {
                return Err(ManifestError::Duplicate(page.id.clone()));
            }
            pages.push(&page.id);
        }
        if self.secrets.len() > SECRETS_MAX {
            return Err(ManifestError::TooManySecrets);
        }
        if self.network.len() > NET_GRANTS_MAX {
            return Err(ManifestError::TooManyNetGrants);
        }
        if let Some(i) = self.network.iter().position(|g| !g.sound()) {
            return Err(ManifestError::BadNetGrant(i));
        }
        if self.log_prefixes.len() > LOG_PREFIXES_MAX {
            return Err(ManifestError::TooManyLogPrefixes);
        }
        if let Some(p) = self.log_prefixes.iter().find(|p| !is_log_prefix(p)) {
            return Err(ManifestError::BadLogPrefix(p.clone()));
        }
        if self.icons.len() > ICONS_MAX {
            return Err(ManifestError::TooManyIcons);
        }
        let mut icons: Vec<&str> = Vec::new();
        for icon in &self.icons {
            if !is_store_key(&icon.name)
                || icons.contains(&icon.name.as_str())
                || icon.svg.len() > ICON_MAX
                || !icon_is_self_contained(&icon.svg)
            {
                return Err(ManifestError::BadIcon(icon.name.clone()));
            }
            icons.push(&icon.name);
        }
        let mut slots: Vec<&str> = Vec::new();
        for slot in &self.secrets {
            if !is_store_key(slot) {
                return Err(ManifestError::BadName(slot.clone()));
            }
            if slots.contains(&slot.as_str()) {
                return Err(ManifestError::Duplicate(slot.clone()));
            }
            slots.push(slot);
        }
        Ok(())
    }

    /// Whether it declares secret slot `slot`.
    pub fn has_secret(&self, slot: &str) -> bool {
        self.secrets.iter().any(|s| s == slot)
    }

    /// Whether it declares page `id`.
    pub fn has_page(&self, id: &str) -> bool {
        self.pages.iter().any(|p| p.id == id)
    }

    /// Whether it receives `addr`.
    pub fn reads(&self, addr: &str) -> bool {
        self.osc_read.iter().any(|p| addr.starts_with(p.as_str()))
    }

    /// Whether it may send to `addr`.
    pub fn writes(&self, addr: &str) -> bool {
        self.osc_write.iter().any(|p| addr.starts_with(p.as_str()))
    }
}

impl Namespace {
    /// Fields, methods and events share one name set.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if !is_namespace_name(&self.name) {
            return Err(ManifestError::BadName(self.name.clone()));
        }
        if self.fields.len() > FIELDS_MAX
            || self.methods.len() > METHODS_MAX
            || self.signals.len() > SIGNALS_MAX
            || self.methods.iter().any(|m| m.params.len() > PARAMS_MAX)
        {
            return Err(ManifestError::TooLarge(self.name.clone()));
        }
        let mut seen: Vec<&str> = Vec::new();
        let names = self
            .fields
            .iter()
            .map(|f| &f.name)
            .chain(self.methods.iter().map(|m| &m.name))
            .chain(self.signals.iter().map(|e| &e.name));
        for name in names {
            if !is_ident(name) {
                return Err(ManifestError::BadName(name.clone()));
            }
            if seen.contains(&name.as_str()) {
                return Err(ManifestError::Duplicate(name.clone()));
            }
            seen.push(name);
        }
        for f in &self.fields {
            check_shape(&f.name, &f.shape)?;
        }
        for e in &self.signals {
            if !is_label(&e.label) {
                return Err(ManifestError::BadName(e.label.clone()));
            }
            if e.payload != Shape::None {
                check_shape(&e.name, &e.payload)?;
            }
        }
        for m in &self.methods {
            let mut params: Vec<&str> = Vec::new();
            for p in &m.params {
                if !is_ident(&p.name) {
                    return Err(ManifestError::BadName(p.name.clone()));
                }
                if params.contains(&p.name.as_str()) {
                    return Err(ManifestError::Duplicate(p.name.clone()));
                }
                params.push(&p.name);
                check_shape(&p.name, &p.shape)?;
            }
            let leaves = m.params.iter().map(|p| p.shape.leaves()).sum::<usize>();
            if leaves > VALUES_MAX {
                return Err(ManifestError::BadShape(m.name.clone()));
            }
            if !m.returns.sound()
                || m.returns.depth() > DEPTH_MAX
                || m.returns.leaves() > VALUES_MAX
            {
                return Err(ManifestError::BadShape(m.name.clone()));
            }
            if let Some(field) = m
                .moves
                .iter()
                .find(|name| !self.fields.iter().any(|f| f.writable && &f.name == *name))
            {
                return Err(ManifestError::MovesUnknown {
                    method: m.name.clone(),
                    field: field.clone(),
                });
            }
        }
        Ok(())
    }

    /// The value a write of `field` delivers: typed, finite and clamped.
    pub fn check_write(&self, field: &str, value: Value) -> Result<Value, CheckError> {
        let f = self
            .fields
            .iter()
            .find(|f| f.name == field)
            .ok_or_else(|| CheckError::UnknownField(field.to_owned()))?;
        if !f.writable {
            return Err(CheckError::ReadOnly(field.to_owned()));
        }
        f.shape.check(&f.name, value)
    }

    /// A value the extension publishes for `field`, if it fits: none
    /// always does.
    pub fn check_published(&self, field: &str, value: Value) -> Option<Value> {
        let f = self.fields.iter().find(|f| f.name == field)?;
        f.shape.check_result(&f.name, value)
    }

    /// The payload the extension sends with signal `name`, if it fits:
    /// none always does.
    pub fn check_signal(&self, name: &str, value: Value) -> Option<Value> {
        let e = self.signals.iter().find(|e| e.name == name)?;
        e.payload.check_result(&e.name, value)
    }

    /// Whether a parameter picks from list `list` ([`Param::options`]).
    pub fn picks_from(&self, list: &str) -> bool {
        self.methods
            .iter()
            .flat_map(|m| &m.params)
            .any(|p| p.options.as_deref() == Some(list))
    }

    /// The data of an item of list `list`, if it fits every parameter
    /// picking from the list, clamped as a call would be.
    pub fn check_item(&self, list: &str, data: Value) -> Option<Value> {
        let mut value = Some(data);
        for p in self
            .methods
            .iter()
            .flat_map(|m| &m.params)
            .filter(|p| p.options.as_deref() == Some(list))
        {
            value = p.shape.check(&p.name, value?).ok();
        }
        value
    }

    /// A reply to `method`, if it fits its result: none always does.
    pub fn check_reply(&self, method: &str, value: Value) -> Option<Value> {
        let m = self.methods.iter().find(|m| m.name == method)?;
        m.returns.check_result(&m.name, value)
    }

    /// The arguments a call of `method` delivers: typed, finite and clamped.
    pub fn check_call(&self, method: &str, args: Vec<Value>) -> Result<Vec<Value>, CheckError> {
        let m = self
            .methods
            .iter()
            .find(|m| m.name == method)
            .ok_or_else(|| CheckError::UnknownMethod(method.to_owned()))?;
        if args.len() != m.params.len() {
            return Err(CheckError::Arity {
                method: method.to_owned(),
                expected: m.params.len(),
                got: args.len(),
            });
        }
        m.params
            .iter()
            .zip(args)
            .map(|(p, v)| p.shape.check(&p.name, v))
            .collect()
    }
}
