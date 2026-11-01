#!/usr/bin/env python3
"""Populate a running CZ4R server with phony data, via its JSON API only.

Creates workers, jobs, job assignments and (for a fraction of assignments)
check-in/out records, so the UI and hours/pay calculations can be tried
against realistic volumes. Needs only the Python 3 standard library.

Examples:
    ADMIN_USER=admin ADMIN_PASSWORD=pass tools/seed_data.py --workers 20 --jobs 100
    tools/seed_data.py --url http://localhost:3000 -u admin -p pass \\
        --workers 50 --jobs 500 --max-workers-per-job 6 --seed 1

    # undo: deletes the seeded jobs and deactivates the seeded workers
    tools/seed_data.py --cleanup

Everything this script creates is tagged (worker names start with
--prefix, job notes start with "[seed]") so --cleanup can find it again.
The API has no hard-delete for workers, so cleanup deactivates them.

Do NOT point this at a production server.
"""

import argparse
import json
import os
import random
import sys
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from datetime import date, timedelta

FIRST = ["Alex", "Sam", "Jordan", "Taylor", "Morgan", "Casey", "Riley", "Jamie",
         "Avery", "Quinn", "Drew", "Robin", "Dana", "Kelly", "Lee", "Pat"]
LAST = ["Nguyen", "Garcia", "Smith", "Okafor", "Kowalski", "Tanaka", "Haddad",
        "Muller", "Silva", "Johnson", "Park", "Rossi", "Ivanov", "Khan"]
STREETS = ["Main St", "Oak Ave", "Maple Dr", "Cedar Ln", "Pine Rd", "Elm Ct"]
SITES = ["Warehouse", "Office Park", "Clinic", "School", "Apartments", "Mall",
         "Depot", "Library", "Plant", "Hotel"]
NOTES = ["", "Bring ladder", "Gate code at front desk", "Call on arrival",
         "Parking in rear", "Access after 8am only"]


class Api:
    def __init__(self, base):
        self.base = base.rstrip("/")
        self.token = None

    def call(self, method, path, body=None, auth=True):
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(self.base + path, data=data, method=method)
        req.add_header("Content-Type", "application/json")
        if auth and self.token:
            req.add_header("Authorization", f"Bearer {self.token}")
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                raw = r.read()
        except urllib.error.HTTPError as e:
            raise RuntimeError(f"{method} {path} -> {e.code}: {e.read().decode(errors='replace')[:300]}")
        return json.loads(raw) if raw else None

    def login(self, user, pw):
        out = self.call("POST", "/api/v1/login", {"username": user, "password": pw}, auth=False)
        if not out.get("token"):
            raise RuntimeError(f"login for {user!r} gave no token (must_change_pw={out.get('must_change_pw')})")
        self.token = out["token"]


def money(rng, lo, hi):
    return f"{rng.uniform(lo, hi):.2f}"


def make_workers(api, rng, n, prefix, password, run_id, pool):
    def one(i):
        name = f"{prefix}{run_id}-{i:04d} {rng.choice(FIRST)} {rng.choice(LAST)}"
        w = {
            "name": name,
            "address": f"{rng.randint(1, 9999)} {rng.choice(STREETS)}",
            "phone": f"555-{rng.randint(100, 999)}-{rng.randint(1000, 9999)}",
            "email": f"seed{run_id}-{i}@example.invalid",
            "hourly": money(rng, 15, 60),
            "mileage": money(rng, 0.3, 0.8),
            "drivetime": money(rng, 10, 30),
            "flatrate": money(rng, 100, 400),
            "admin": False,
        }
        wid = api.call("POST", "/admin/api/v1/create-worker", w)["id"]
        # New workers must set a password before they can log in.
        api.call("POST", "/api/v1/change-pw",
                 {"id": wid, "password1": password, "password2": password}, auth=False)
        return wid, name

    # Pre-draw nothing random inside threads' shared rng use: Random is
    # thread-safe enough for this (individual calls are atomic under the GIL),
    # only reproducibility of --seed across thread interleavings is lost for
    # workers, so run them serially when a seed was given.
    return list(pool.map(one, range(n)))


def make_jobs(api, rng, n, worker_ids, max_per_job, days, pool):
    today = date.today()
    # Draw all the randomness up front, in order, so --seed is reproducible.
    specs = []
    for i in range(n):
        k = rng.randint(0, min(max_per_job, len(worker_ids))) if worker_ids else 0
        assigned = rng.sample(worker_ids, k)
        specs.append({
            "sitename": f"{rng.choice(SITES)} {rng.randint(1, 99)}",
            "servcode": f"SC{rng.randint(100, 999)}",
            "workorder": f"WO-{rng.randint(10000, 99999)}",
            "address": f"{rng.randint(1, 9999)} {rng.choice(STREETS)}",
            "date": (today + timedelta(days=rng.randint(-days, days // 4))).isoformat(),
            "notes": ("[seed] " + rng.choice(NOTES)).strip(),
            "assignments": [{"worker": w, "flat_rate": rng.random() < 0.2} for w in assigned],
        })

    def one(spec):
        return api.call("POST", "/admin/api/v1/jobs", spec)["job_id"], spec

    return list(pool.map(one, specs))


def make_checkins(api, rng, jobs, fraction, pool):
    today = date.today().isoformat()
    reqs = []
    for job_id, spec in jobs:
        if spec["date"] > today:
            continue  # future jobs haven't happened yet
        for a in spec["assignments"]:
            if rng.random() > fraction:
                continue
            start = rng.randint(6 * 60, 12 * 60)
            end = min(start + rng.randint(60, 10 * 60), 23 * 60 + 59)
            driving = rng.random() < 0.6
            reqs.append({
                "job_id": job_id,
                "worker_id": a["worker"],
                "signin": f"{start // 60:02d}:{start % 60:02d}",
                "signout": f"{end // 60:02d}:{end % 60:02d}",
                "miles_driven": round(rng.uniform(1, 80), 1) if driving else 0,
                "hours_driven": float(rng.randint(0, 2)) if driving else 0,
                "minutes_driven": float(rng.choice([0, 15, 30, 45])) if driving else 0,
                "extra_expenses_cents": rng.choice([0, 0, 0, 500, 1250, 4000]),
                "notes": rng.choice(["", "", "Done", "Customer was late", "Needed extra parts"]),
            })
    # Admins may update any worker's record via this endpoint.
    list(pool.map(lambda r: api.call("POST", "/api/v1/checkinout", r), reqs))
    return len(reqs)


def cleanup(api, prefix, pool):
    users = api.call("GET", "/admin/api/v1/users")
    seeded = [u for u in users if u["name"].startswith(prefix) and not u["deactivated"]]
    # /joblist is the admin's view of jobs; it returns one row per
    # (job, worker), so de-duplicate by job id. Use a wide date range since
    # it otherwise defaults to a narrow window.
    q = urllib.parse.urlencode({"start_date": "2000-01-01", "end_date": "2100-01-01"})
    rows = api.call("GET", "/api/v1/joblist?" + q)["jobs"]
    seeded_jobs = [{"id": i} for i in {r["job_id"] for r in rows if r["notes"].startswith("[seed]")}]
    list(pool.map(lambda j: api.call("DELETE", f"/admin/api/v1/jobs/{j['id']}"), seeded_jobs))
    list(pool.map(lambda u: api.call("POST", "/admin/api/v1/deactivate-worker", {"user": u["id"]}), seeded))
    print(f"deleted {len(seeded_jobs)} jobs, deactivated {len(seeded)} workers")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--url", default=os.environ.get("CZ4R_URL", "http://localhost:3000"))
    ap.add_argument("-u", "--user", default=os.environ.get("ADMIN_USER", "admin"))
    ap.add_argument("-p", "--password", default=os.environ.get("ADMIN_PASSWORD"))
    ap.add_argument("--workers", type=int, default=10)
    ap.add_argument("--jobs", type=int, default=50)
    ap.add_argument("--max-workers-per-job", type=int, default=4)
    ap.add_argument("--checkin-fraction", type=float, default=1.0,
                    help="fraction of past assignments given random sign-in/out times (default 1.0 = all; future jobs are always left blank)")
    ap.add_argument("--days", type=int, default=90, help="spread job dates over this many past days")
    ap.add_argument("--worker-password", default="seedpass",
                    help="password given to every created worker")
    ap.add_argument("--prefix", default="SEED-", help="name prefix marking seeded workers")
    ap.add_argument("--seed", type=int, help="RNG seed for reproducible data")
    ap.add_argument("--threads", type=int, default=8)
    ap.add_argument("--cleanup", action="store_true",
                    help="delete seeded jobs and deactivate seeded workers, then exit")
    args = ap.parse_args()

    if not args.password:
        sys.exit("admin password required: pass -p or set ADMIN_PASSWORD")

    api = Api(args.url)
    api.login(args.user, args.password)
    # A seed means "reproducible": avoid thread-interleaving in anything random.
    pool = ThreadPoolExecutor(1 if args.seed is not None else args.threads)

    if args.cleanup:
        cleanup(api, args.prefix, pool)
        return

    rng = random.Random(args.seed)
    run_id = f"{rng.randint(0, 0xFFFFFF):06x}"

    workers = make_workers(api, rng, args.workers, args.prefix, args.worker_password, run_id, pool)
    print(f"created {len(workers)} workers (password {args.worker_password!r}), e.g. {workers[0][1]!r}" if workers else "no workers")
    jobs = make_jobs(api, rng, args.jobs, [w[0] for w in workers], args.max_workers_per_job, args.days, pool)
    print(f"created {len(jobs)} jobs with {sum(len(s['assignments']) for _, s in jobs)} assignments")
    n = make_checkins(api, rng, jobs, args.checkin_fraction, pool)
    print(f"recorded {n} check-in/out entries")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, urllib.error.URLError) as e:
        sys.exit(f"error: {e}")
