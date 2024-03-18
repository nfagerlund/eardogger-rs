use crate::{
    db::{Dogear, Token, User},
    util::Pagination,
};
use minijinja::{context, Template};
use serde::Serialize;

// ^^ I'm gonna just always refer to minijinja::Environment by its full
// name, because the bare name is confusing.

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
struct Common<'a> {
    title: &'a str,
    user: Option<&'a User>,
    csrf_token: &'a str,
}

#[derive(Serialize)]
struct TokensList<'a> {
    tokens: &'a [Token],
    pagination: Pagination,
}

#[derive(Serialize)]
struct DogearsList<'a> {
    dogears: &'a [Dogear],
    pagination: Pagination,
}

#[derive(Serialize)]
struct MarkedPage<'a> {
    updated_dogears: &'a [Dogear],
    bookmarked_url: &'a str,
    slowmode: bool,
}

#[derive(Serialize)]
struct CreatePage<'a> {
    bookmarked_url: &'a str,
}

// This one's kind of silly, but my theory is that I'll benefit if everything
// *inside* the freeform context has a known type.
#[derive(Serialize)]
struct ErrorPage<'a> {
    error: &'a str,
}

// TODO still: bookmarklets.
