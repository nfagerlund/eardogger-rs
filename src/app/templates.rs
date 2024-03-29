use crate::{
    db::{Dogear, Token, TokenScope, User},
    util::{Pagination, SHORT_DATE},
};
use minijinja::{escape_formatter, Value};
// ^^ always gonna qualify minijinja::Environment bc its name is confusing
use serde::Serialize;
use time::{format_description::well_known::Iso8601, OffsetDateTime};

/// A template filter for turning an ISO8601 timestamp into a short date like 2024-03-22.
/// If the timestamp can't parse or lacks date elements, we default to just displaying
/// whatever we've got.
fn short_date(date_str: &str) -> String {
    let Ok(date) = OffsetDateTime::parse(date_str, &Iso8601::DEFAULT) else {
        return date_str.to_string();
    };
    date.format(SHORT_DATE)
        .unwrap_or_else(|_| date_str.to_string())
}

/// A template filter for translating token scopes to explanatory text.
fn explain_scope(scope_str: &str) -> &'static str {
    match TokenScope::from(scope_str) {
        TokenScope::WriteDogears => "Can mark your spot.",
        TokenScope::ManageDogears => "Can view, update, and delete dogears.",
        TokenScope::Invalid => "Cannot be used.",
    }
}

/// A replacement for minijinja's built-in `default` filter, which will
/// replace an undefined value but doesn't usefully handle None values.
/// This filter handles both kinds of nothing.
fn unwrap_or(v: Value, other: Option<Value>) -> Value {
    if v.is_undefined() || v.is_none() {
        other.unwrap_or_else(|| Value::from(""))
    } else {
        v
    }
}

// Right, so here's my theory of template data (today, for now). Making a
// struct for every page would be kind of silly and a pain, so I do in fact
// want the flexibility of that context!{k=>} macro. However, I also want type
// guardrails wherever it makes sense to get them. SO, I'm gonna make structs
// for each "major chunk of page stuff" basically, and then assemble those into
// a context so that each context has maybe two or three things in it tops.

/// Template data that pretty much every page is gonna need. The main layout
/// is free to use anything in here, as are any nested pages. Haven't decided
/// about fragments yet.
#[derive(Serialize)]
pub struct Common<'a> {
    pub title: &'a str,
    pub user: Option<&'a User>,
    pub csrf_token: &'a str,
}

impl<'a> Common<'a> {
    /// Make a Common args with no user and an invalid csrf token. This
    /// is for pages that can be viewed while logged out, without turning
    /// into a login form.
    pub fn anonymous(title: &'a str) -> Self {
        Self {
            title,
            user: None,
            csrf_token: "invalid",
        }
    }
}

#[derive(Serialize)]
pub struct TokensList<'a> {
    pub tokens: &'a [Token],
    pub pagination: Pagination,
}

#[derive(Serialize)]
pub struct DogearsList<'a> {
    pub dogears: &'a [Dogear],
    pub pagination: Pagination,
}

#[derive(Serialize)]
pub struct MarkedPage<'a> {
    pub updated_dogears: &'a [Dogear],
    pub bookmarked_url: &'a str,
    pub slowmode: bool,
}

#[derive(Serialize)]
pub struct CreatePage<'a> {
    pub bookmarked_url: &'a str,
}

#[derive(Serialize)]
pub struct LoginPage<'a> {
    pub return_to: &'a str,
    pub previously_failed: bool,
}

// This one's kind of silly, but my theory is that I'll benefit if everything
// *inside* the freeform context has a known type.
#[derive(Serialize)]
pub struct ErrorPage<'a> {
    pub error: &'a str,
}

// TODO still: bookmarklets.

// For now, I'm just gonna load all the templates statically and compile em
// in to the app.
#[tracing::instrument]
pub fn load_templates() -> anyhow::Result<minijinja::Environment<'static>> {
    let mut env = minijinja::Environment::new();
    // Bookmarklets:
    env.add_template("mark.js.j2", include_str!("../../bookmarklets/mark.js.j2"))?;
    env.add_template(
        "where.js.j2",
        include_str!("../../bookmarklets/where.js.j2"),
    )?;

    // HTML views:
    env.add_template(
        "_layout.html.j2",
        include_str!("../../templates/_layout.html.j2"),
    )?;
    env.add_template(
        "account.html.j2",
        include_str!("../../templates/account.html.j2"),
    )?;
    env.add_template(
        "create.html.j2",
        include_str!("../../templates/create.html.j2"),
    )?;
    env.add_template(
        "error.html.j2",
        include_str!("../../templates/error.html.j2"),
    )?;
    env.add_template("faq.html.j2", include_str!("../../templates/faq.html.j2"))?;
    env.add_template(
        "fragment.dogears.html.j2",
        include_str!("../../templates/fragment.dogears.html.j2"),
    )?;
    env.add_template(
        "fragment.tokens.html.j2",
        include_str!("../../templates/fragment.tokens.html.j2"),
    )?;
    env.add_template(
        "index.html.j2",
        include_str!("../../templates/index.html.j2"),
    )?;
    env.add_template(
        "install.html.j2",
        include_str!("../../templates/install.html.j2"),
    )?;
    env.add_template(
        "login.html.j2",
        include_str!("../../templates/login.html.j2"),
    )?;
    env.add_template(
        "macro.pagination.html.j2",
        include_str!("../../templates/macro.pagination.html.j2"),
    )?;
    env.add_template(
        "marked.html.j2",
        include_str!("../../templates/marked.html.j2"),
    )?;
    env.add_filter("short_date", short_date);
    env.add_filter("explain_scope", explain_scope);
    // It's actually possible to just replace `default` by name in the environment,
    // but I want to make sure the differing expectations are recorded for future
    // maintenance.
    env.add_filter("unwrap_or", unwrap_or);
    // By default, minijinja prints None values as the literal string
    // "none". This is apparently intentional, but I extremely don't want it.
    // Luckily, the formatter provides a clean way to patch that for the whole
    // runtime, instead of having to do per-type serialize shenanigans. We want
    // the default escape_formatter for file-extension mediated HTML escaping,
    // but we'll wrap it with a lil closure to fix Nones. Note that this ONLY
    // affects printing; values are preserved in a typed format when passing
    // through filters or functions.
    env.set_formatter(|out, state, value| {
        escape_formatter(
            out,
            state,
            if value.is_none() {
                &Value::UNDEFINED
            } else {
                value
            },
        )
    });
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use minijinja::context;

    // Using an embedded template to avoid brittleness with actual
    // template text that might change over time.
    #[test]
    fn bookmarklet_escaping() {
        let mut env = load_templates().expect("loads ok");
        env.add_template(
            "test.js.j2",
            r##"(() => { document.location.href = {{ own_origin }} + '/resume/' + encodeURIComponent(location.href); })();"##
        ).expect("added ok");

        let ctx = context! {
            own_origin => "https://eardogger.com",
        };
        let res = env
            .get_template("test.js.j2")
            .expect("got ok")
            .render(ctx)
            .expect("rendered ok");
        // The json auto-formatter QUOTES the input string when interpolating it:
        let expected = r##"(() => { document.location.href = "https://eardogger.com" + '/resume/' + encodeURIComponent(location.href); })();"##;
        assert_eq!(res, expected);
        let bookmarklet = crate::util::make_bookmarklet(&res);
        // haha ok, it looks hella nasty, but anyway, I tested this in a browser
        // and it works as expected.
        let expected_bmkt = r#"javascript:(()%20%3D%3E%20%7B%20document.location.href%20%3D%20%22https%3A%2F%2Feardogger.com%22%20%2B%20'%2Fresume%2F'%20%2B%20encodeURIComponent(location.href)%3B%20%7D)()%3B"#;
        assert_eq!(bookmarklet, expected_bmkt);
    }
}
