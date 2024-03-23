use crate::{
    db::{Dogear, Token, User},
    util::Pagination,
};
use minijinja::{context, Template};
// ^^ always gonna qualify minijinja::Environment bc its name is confusing
use serde::Serialize;
use time::{
    format_description::{well_known::Iso8601, FormatItem},
    macros::format_description,
    OffsetDateTime,
};

const SHORT_DATE: &[FormatItem] =
    format_description!("[year]-[month repr:numerical padding:none]-[day padding:none]");

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
pub fn load_templates() -> anyhow::Result<minijinja::Environment<'static>> {
    let mut env = minijinja::Environment::new();
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
    Ok(env)
}
