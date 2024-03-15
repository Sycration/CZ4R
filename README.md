# CZ4R

Version 1.1.2

CZ4R is a time tracking software written in plain HTML (with Bootstrap) and Rust. It aims to use as little javascript as possible.

To run:

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

If you want to run CZ4R without docker, this is possible too, just set the environment variables seen in `.devcontainer/.env.template` yourself, and point it at an instance of Postgresql 14.

The web UI will be exposed on port 3000 by default.