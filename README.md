# Eardogger 2024

The same service as https://github.com/nfagerlund/eardogger, except I'm rewriting the whole thing with Rust, SQLite+sqlx, axum/tower/hyper, and a secret ingredient.

## Why Rewrite It In Rust?

Don't worry about it. ☺️

- **Not performance.** Eardogger don't do all that much, bless its heart, so it could be built in whatever.
- **It's mostly about sqlite.** The web's premier unpopular bookmarking service is never going to get crowded enough to justify owning a Postgres instance forever, so that dependency is just a seabird necklace.
- **It's mostly about operational agency.** I'm doing some semi-experimental shit to produce a hybrid-mode app that can both particpate in the modern "lots of nice stuff" Rust http ecosystem, and ride the bus on cheap shared hosting. That second mode teases a potentially massive payout for any lightly-maintained, low-traffic, unpopular app: the impossible dream of good-enough performance, zero marginal cost, and zero net-new infrastructure.
- **It's mostly about whatever occurs to me next.** These are currently the tools I'm most interested in, so they're what I'm gonna reach for the next time I get a random brainstorm and feel like making a web toy that has a backend. This lets me get familiar with them in a domain where I already understand the core app logic.

## I Heard U Dinked With Some Edge-Case Semantics in the V1 API Without Bumping the Version???

Hush up!!

## Notes for future etc.

### Options, config file

yeah

### Database

Before you can do literally anything, you need a sqlite database that's been set to WAL mode. Easy enough, though:

```
sqlite3 dev.db
PRAGMA journal_mode = WAL;
.exit
```

Also your config file needs to be pointing at the DB file.

#### Compilation

We're using sqlx macros for type-checked queries, and that means you can't compile the app at all unless you have `DATABASE_URL` pointed at a fully migrated database file. (You can use a `.env` file (gitignored) to persistently set this.)

I'm gonna do the "prepare" thing for easier compilation, but haven't done it yet.

#### Migrations

We use [sqlx CLI](https://lib.rs/crates/sqlx-cli) for database migrations.

We're using the sqlx library features to do our own built-in support for the simplest path — run with `--migrate` to run pending migrations, set `validate_migrations` in the config file for a startup-time safety check, run with `--status` to see the deets, etc.

But for any nastier form of db repair, you'll want the sqlx CLI itself and a copy of the migrations dir from the source.
