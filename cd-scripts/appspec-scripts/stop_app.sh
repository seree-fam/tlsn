#!/bin/bash
set -ex

echo "APPLICATION_NAME: $APPLICATION_NAME"
APP_NAME=$(echo $APPLICATION_NAME | awk -F- '{ print $2 }')
echo "APP_NAME: $APP_NAME"

# List all processes for debugging
echo "Listing all processes:"
ps -ef | grep -v grep

if [ "$APP_NAME" = "stable" ]; then
  VERSIONS_DEPLOYED=$(find ~/ -maxdepth 1 -type d -name 'stable_*')
  VERSIONS_DEPLOYED_COUNT=$(echo $VERSIONS_DEPLOYED | wc -w)

  # Remove oldest version if exists
  if [ $VERSIONS_DEPLOYED_COUNT -eq 3 ]; then
    echo "Candidate versions to be removed:"
    OLDEST_DIR=""
    OLDEST_TIME=""

    for DIR in $VERSIONS_DEPLOYED; do
      TIME=$(stat -c %W $DIR)

      if [ -z "$OLDEST_TIME" ] || [ "$TIME" -lt "$OLDEST_TIME" ]; then
        OLDEST_DIR=$DIR
        OLDEST_TIME=$TIME
      fi
    done

    echo "The oldest version is running under: $OLDEST_DIR"
    PID=$(lsof $OLDEST_DIR/tlsn/notary/target/release/notary-server | awk '{ print $2 }' | tail -1)
    if [ -z "$PID" ]; then
      echo "No process found for $OLDEST_DIR. This might be the first deployment."
    else
      kill -15 $PID || true
    fi
    rm -rf $OLDEST_DIR
  fi
else
  PID=$(pgrep -f 'notary.*$APP_NAME')
  if [ -z "$PID" ]; then
    echo "No process found for $APP_NAME. This might be the first deployment."
  else
    kill -15 $PID || true
  fi
fi

exit 0