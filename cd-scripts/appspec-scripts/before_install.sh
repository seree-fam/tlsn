#!/bin/bash
set -e

echo "Starting before_install.sh"
echo "APPLICATION_NAME: $APPLICATION_NAME"

APP_NAME=$(echo $APPLICATION_NAME | awk -F- '{ print $2 }')
echo "APP_NAME: $APP_NAME"

if [ "$APP_NAME" = "nightly" ]; then
  echo "Creating directory for nightly build"
  if [ ! -d ~/$APP_NAME ]; then
    mkdir ~/$APP_NAME
  fi
fi

echo "before_install.sh completed successfully"
exit 0