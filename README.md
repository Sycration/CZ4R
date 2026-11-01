# CZ4R

Version 1.2.3

CZ4R is a time tracking software written in plain HTML (with Bootstrap) and Rust. It aims to use as little javascript as possible.

See [https://github.com/Sycration/CZ4R/wiki/Installation-and-setup](https://github.com/Sycration/CZ4R/wiki/Installation-and-setup) to get started
## Seeding test data

`tools/seed_data.py` fills a running server with phony workers, jobs, assignments and check-in/out records through the JSON API (Python 3 stdlib only):

```
ADMIN_PASSWORD=pass tools/seed_data.py --workers 20 --jobs 100
tools/seed_data.py --cleanup   # delete seeded jobs, deactivate seeded workers
```

See `tools/seed_data.py --help` for options. Don't run it against production.
