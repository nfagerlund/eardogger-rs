#!/bin/bash

# Sqlite database backup script for cron jobs. You'd make multiple copies of
# this and embed the credentials and bucket names in the script (lock down
# those permissions!) so that all the important stuff is outside the crontab.
#
# This approach is meant for the "running on a permanent web server" paradigm;
# you'd need to do something else in a serverless scenario.
#
# This is pretty slapped-together, but eh it works. MAILTO in the crontab lets
# you monitor things in a dedicated mailbox. Later I wanna build a dead man's
# switch service that serves weekly summaries by JSON-feed.

# - sqlite .backup preserves journal_mode etc. etc., whereas VACUUM INTO doesn't
# necessarily.

# - b2 buckets are always versioned (unlike s3), so uploading the same filename
# keeps the old versions around. The bucket should be configured with lifecycle
# rules to discard old db backups after enough time has passed.
# Figure do both a daily and a monthly backup job for a big rolling window --
# daily is for computer death, monthly is for corruption of some kind.

set -e
set -o pipefail

# eardogger-dev b2 app key
B2_APPLICATION_KEY_ID="your key id"
B2_APPLICATION_KEY="your key"

B2_BUCKET="your bucket"

DB_FILE="/home/YOU/eardogger-dev-datadir/dev.db"
# filename prefix is meaningful for bucket lifecycle rules:
BACKUP_FILENAME="daily-eardogger-dev.db"
TMP_DIR="/home/YOU/tmp"
BACKUP_FILE="${TMP_DIR}/${BACKUP_FILENAME}"

# waste any old backups from failed runs
rm -f $BACKUP_FILE "${BACKUP_FILE}.gz"

/usr/bin/sqlite3 $DB_FILE ".backup ${BACKUP_FILE}"

/usr/bin/gzip -f $BACKUP_FILE

# I had to "pip3 install --user b2" for this, bc the premade binary expects /tmp to
# allow execution and turns out the server don't play that.
/home/YOU/.local/bin/b2 file upload --no-progress $B2_BUCKET "${BACKUP_FILE}.gz" "${BACKUP_FILENAME}.gz"

rm -f $BACKUP_FILE "${BACKUP_FILE}.gz"
