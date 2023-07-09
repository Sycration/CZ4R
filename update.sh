#!/usr/bin/env sh
docker-compose up --force-recreate --build -d
docker image prune -f