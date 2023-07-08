# CZ4R

CZ4R is a time tracking software written in plain HTML (with Bootstrap) and Rust. It aims to use as little javascript as possible.

It is currently in active development. Not all features are complete. Do not use in production. Cavē.

First, clone the repo.
```
git clone https://github.com/Sycration/CZ4R
cd CZ4R
```

To configure, copy `.devcontainer/.env.template` to `.devcontainer/.env` and modify the values in there to your liking. 

Next, to run CZ4R, use `docker-compose`.

```
docker-compose up -d
```

The web UI will be exposed on port 3000 by default.

Documentation on the use of the software is in progress.