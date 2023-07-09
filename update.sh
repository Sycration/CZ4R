#!/usr/bin/env sh
git pull
docker-compose up --force-recreate --build -d
docker image prune -f