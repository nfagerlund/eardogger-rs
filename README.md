# Eardogger 2

Eardogger is a movable bookmarks service, for reading serialized content on the web. [It runs as a free service at Eardogger.com](https://eardogger.com).

This is a full rewrite (2024) of the original (https://github.com/nfagerlund/eardogger/).

## Top-level app logic

- Dogears are bookmarks that act like a cursor. Their permanent identifier is a URL prefix, but the full URL they point to can change frequently.
- To update a dogear, send a URL. Any dogears whose prefixes match it will update.
- The main interaction point is a javascript bookmarklet that identifies a user via a baked-in token and sends the current tab's URL.
    - I had a vague ambition to make a webextension and a native iOS share sheet app at some point, but those haven't felt necessary.

## Routes and behaviors

- In the v1 readme, I hand-wrote a bunch of documentation for all the service's API and web routes, but as fortold, it ended up kind of falling out of date, and I don't want to go that route again.
    - Honestly half the point of using a framework like axum or express is that the route handler code is short and expressive enough that I can still read it fluently when returning after several years. So let's not bother with manual docs again. No one's using the API here anyhow.
        - (But if you *want* to use the API for something, do please hit me up and I'll write something.)
- There's several API routes that can be hit with either session cookie auth or limited-scope token auth. The site itself uses a few of these, but "update" is the only one used by the bookmarklet (and thus the only one that allows CORS).
    - API routes expect and return `application/json`.
- There's some shared pagination behavior for list endpoints.

## Infrastructure and operations

Pro-tip for the forgetful and distracted: always write down your prod infrastructure layout in the first place you'll look for it. A bunch of this feels like oversharing, but... you'd need to be able to log in as me in several places to mess with me, so 🤷🏽

### General

Eardogger v2 runs as a single service. It uses Sqlite for its database, and keeps a db file in a configurable place on an attached local storage volume.

The service can run in two modes: FCGI, and HTTP.

- FCGI mode lets me sneak the production-scale app into shared hosting scenarios that most people would only consider suitable for PHP or CGI scripts. At the moment it's the intended long-term deployment mode, because my theory is that it'll allow hands-off operation and exploit existing infrastructure that I need to possess anyway (and which is mostly sysadminned by _not me_).
- HTTP mode hedges my bets. It lets the app run as a standalone process behind a TLS-terminating reverse proxy. I could deploy it on a fly.io machine or whatever for cheap or free.

### Deploying

Since I'm using fcgi mode and running on a _normal-ass web server,_ I'm currently being an absolute caveboi about this. Build on local system, upload a tarball, SSH in, and party.

Make sure your lappy's rust environment can cross-compile for `x86_64-unknown-linux-gnu`. Using gnu libc seems to result in smaller binaries and maybe better perf than using musl, but it DOES require installing additional toolchain bullshit. There's some notes in FMP/incantations about all that, and [this](https://github.com/SergioBenitez/homebrew-osxct) seems to be where I ended up.

- `./release.sh` (build and tar some stuff)
- `scp eardogger-release.tar.gz nfagerlund@nfagerlund.net:~/`
- `ssh nfagerlund@nfagerlund.net`
- `cd eardogger-prod-datadir`
- `tar -xzf ../eardogger-release.tar.gz`
    - — you wanna unzip in-place for updated `public/` and example files and stuff.
- `./eardogger-rs --check` to validate config and check migrations.
- `./eardogger-rs --migrate` if there's migrations.
    - If the migrations are somehow _fucked_ instead of just behind, you'll need the `sqlx migrate` command (not included, but you can `cargo install` it and it's already available for the nfagerlund DH server user).
- `cp eardogger-rs ~/bin/`
- `killall eardogger-rs`

Something to keep an eye on: On my first prod deploy, I was having a bit of trouble writing over `~/bin/eardogger-rs` because it was in-use. Depending on the amount of traffic, I might need to tweak the deploy process to do something like (redirect .htaccess to binary in datadir) -> killall -> (cp over the version in `~/bin`) -> (update .htaccess again) -> killall. 🤨

### Historical

See [the eardogger v1 README](https://github.com/nfagerlund/eardogger/). As of June 2024, the fly.io app configs are still present but are scaled to 0 instances, and their reserved ipv4s are released. The Neon databases are still present, but nothing should be accessing them; I should probably get around to wasting those at some point.

### Prod

- https://eardogger.com!
- Hosting: It's on my DreamHost shared hosting, tied to the nfagerlund server user.
    - I'm not posting the global DreamHost "panel" account name publicly here, as that's a bit more sensitive. (If someone has to get in to maintain the site after something happens to me, well... hopefully you've gotten access to my password vault; everything is in there.)
    - Web dir: `~/eardogger.com`
        - The `.htaccess` file is where the magic happens.
            - This also has the "no www." rule.
        - Also, a mandatory `index.html` file. (Only shown during downtime, but also it prevents DH from showing a placeholder page.)
    - Data/config: `~/eardogger-prod-datadir`
        - DB: eardogger.db
    - App logs: `~/logs-eardogger-prod`
        - Apache logs are in `~/logs/eardogger.com`
- DNS: Dreamhost.
    - Registration for eardogger.com is thru Hover, but it's pointed at DH's DNS servers.
    - There's still rules in Hover to point at the old fly.io allocation, but they're inert as long as we're not using Hover's nameservers.
- TLS: DreamHost / Let's Encrypt
    - Configured through the DH panel.
- Monitoring: uptimerobot on a 30m interval.
- Backups: Daily and monthly cron jobs to send backups to Backblaze B2.
    - `crontab -e` to configure
    - Scripts are in `~/cronbin`
    - Bucket names are considered a secret, since I don't want randos slammin' em (even though they're set to allPrivate).
    - Dedicated buckets and write-only app keys for prod and dev.
    - Server-side encrypt at rest.
    - Rolling window for file version lifetimes; currently 6mo for monthly and 30d for daily.
    - There's Terraform code for the B2 bucket + key objects, but it's purely local on my machine, stored at `tf-eardogger2-b2-backups`. The app key for provisioning is set to expire eventually, but you can just make a new one in the b2 UI.
    - Cron output gets emailed to a predictably named mailbox on my personal domain, can be viewed w/ DH webmail... but I wanna come up with a dead-man's switch thing to give me weekly summaries as a json-feed. Next project, codename: GRAVES.

### Dev/soak

- https://eardogger-dev.nfagerlund.net/
- Hosting: DreamHost, under the nfagerlund server user.
    - Web dir: `~/eardogger-dev.nfagerlund.net`.
        - `.htaccess` file, plus mandatory `index.html`.
        - **To put the dev server to sleep** when not actively developing on it, you can comment out the relevant `.htaccess` bits.
    - Data/config: `~/eardogger-dev-datadir`
        - This includes the database file!
    - App logs: `~/logs-eardogger-dev`
        - Apache logs are in `~/logs/eardogger-dev.nfagerlund.net`,
- DNS: DreamHost.
    - The registration for the main nfagerlund.net domain is through Hover, but it's configured to use DreamHost's DNS servers.
    - DreamHost's "sites" panel can configure new subdomains for a domain that's already aimed at it.
- TLS: DreamHost / Let's Encrypt
    - Configured through the DH panel.
- Monitoring: None.
- Backups: Turned off, but there's a commented-out cron script for it.

## Run-time stuff

Eardogger requires 100% of the following shit:

- A config file
- A database file
- An encryption key for signed cookies
- A "public" dir with its static assets (js/css)

### Data dir

Generally that all goes in a single directory. As a shortcut, we assume the directory with the config file is the data dir with all the other stuff, so any relative file paths in the config get resolved from there.

But that's optional; you can use absolute paths in the config and put stuff wherever.

### CLI options

Canonical info is over in src/args.rs, but here's a (manual, might drift out of date) reminder.

- `--config FILE` — specify a config file. If omitted, the app will try to load `eardogger.toml` from the CWD, but that's just a shortcut for local dev.
- `--version` — print version info and bail.
- `--check` or `--status` — load the config file, connect to the database file, print the status of migrations (so you can tell whether any are pending), and bail.
- `--migrate` — perform any pending db migrations and bail.

### Config file

The config file is mandatory and nearly all of the fields in it are mandatory. Keeps things simple.

The `eardogger.example.toml` file is the canonical docs for all this, and it should have comments explaining everything. There's a test that ensures the example config stays complete and valid.

### Cookie key file

It's however many bytes of random bullshit and we generate it automatically on first run if it doesn't already exist. Don't sweat it.

We only use this for the login/signup form anti-CSRF cookie scheme. Our normal session cookies are not signed or encrypted; they're just a single random identifier so it'd be redundant.

### Database

Before you can do literally anything, you need a sqlite database that's been set to WAL mode. Easy enough, though:

```
sqlite3 dev.db
PRAGMA journal_mode = WAL;
.exit
```

Also your config file needs to be pointing at the DB file.

### Migrations

We're using sqlx's database migration features.

- The [sqlx-cli](https://lib.rs/crates/sqlx-cli) crate has most of the docs about this.
- `sqlx migrate add name-of-migration` to make a new migration.
- `eardogger-rs --migrate` or `sqlx migrate run` to perform migrations.
    - The built-in migrate command uses a copy of the migrations embedded in the binary, so you don't need the migrations dir for that.
- `eardogger-rs --check` to see the status of em.
- We validate migrations at server startup if `validate_migrations` is set in the config file. You might want to turn it off if you're starting up constantly, but so far the performance impact seems unmeasurably small. Probably gets bigger if you have a ton of migrations.
- For any nastier form of db repair, you'll want the sqlx CLI itself and a copy of the migrations dir from the source. The deployment tarball includes the migrations.

### Data import

Eardogger v1 and v2 use different databases with different sql dialects and slightly different schemae, and attempting to export -> normalize -> import _actual sql statements_ (or honestly even CSV) sounded like an awful time. So we got a lil migration script:

- `cargo build --release --features postgres-import --bin postgres-import`
- `./target/release/postgres-import --sqlite_url URL --postgres_url URL`

It's stashed behind a cargo feature so we can usually skip the sqlx postgres dep.

You need to create a sqlite db in WAL mode before running it.

It can work with an existing destination database, and will update existing records when possible... but that's just to make it more resilient during a one-time migration that encounters hiccups. It's not meant to be run on a regular basis against an in-service DB, and could result in data corruption if someone created a conflicting username since the last run or something.

There's also a `--revert-to-postgres` option for reversing the polarity. I haven't really tested it.

### FCGI mode and Apache configs

The main hosting environment for fcgi mode is Apache with mod_fcgid.

The way this works is, you have a location associated with the app's domain, and then you "mount" the app there by telling Apache to handle every URL that would land inside that location via the same fcgid "wrapper script" (i.e. our app).

In shared hosting, you usually don't have access to normal Apache configs, so you probably have a directory on disk associated with a VirtualHost that you requested indirectly via an admin panel interface. In that case, you'll be using an `.htaccess` file to configure all this. (So, yeah: to run in shared hosting, you must have the ability to do `.htaccess` overrides, and the server must have mod_fcgid enabled.)

The `htaccess-example` file in this repo has the details.

Some other stuff you might run into:

- On DreamHost, I needed a dummy index.html file in the site's root dir to turn off the interposed "coming soon!!" page that dreamhost does if you haven't uploaded anything. The `.htaccess` rules keep the page from ever being shown, but it's got to exist anyway.
- You must have the `AllowEncodedSlashes NoDecode` directive set on your domain's VirtualHost, for the /mark/:url endpoint to work. (Note that some apache versions don't properly inherit a global value into vhosts.) DreamHost has this set by default, it seems, but I had to add it in my local playground. Unfortunately, you CANNOT set this in .htaccess; it has to be in the real configs. (On further consideration, I might want to switch the /mark/ endpoint to prefer using a query param, for more reliable compatibility in the future.)
- Logging: mod_fcgid connects both the stderr and stdout of your process to the apache server's MAIN ErrorLog, NOT your vhost's log. So you want to be EXTREMELY quiet on your main pipes if you're running on a shared host. Set `stdout = false` in your app config.

## Development

### Tests

Plain old `cargo test`.

I salvaged and ported the vast majority of the existing test case logic from eardogger v1, because those tests saved my bacon a couple times and I felt I owed it to Future Nick.

### Compilation

We're using sqlx macros for type-checked queries, so it needs database info DURING compilation. Pretty wild!

There's two ways to provide that:

- Point the `DATABASE_URL` env var at a fully migrated database file. (You can use a `.env` file (gitignored) to persistently set this.)
- Offline mode with cached query data in the `.sqlx` directory: leave `DATABASE_URL` unset, or else set `SQLX_OFFLINE="true"` to override it. No db file needed, appropriate for CI builds etc.

To update the cached query information, you have to run the following two commands:

```
cargo sqlx prepare
cargo sqlx prepare -- --tests
```

(In the latter, `--tests` ends up being passed to rustc.)

I'm gonna try running for a while with a valid `DATABASE_URL` but with `SQLX_OFFLINE=true` — my theory is that this'll prevent me from accidentally leaving the project in an unbuildable state due to cached queries lagging behind reality.

UPDATE: Yeah, running that way is annoying if you're frequently tweaking queries, so it's worth turning offline off for a minute if you're doing a bunch of db stuff. But it's a solid default position for protecting yourself from forgetfulness.
