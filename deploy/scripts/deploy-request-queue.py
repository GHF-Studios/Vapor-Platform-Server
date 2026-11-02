#!/usr/bin/env python3
"""Small transaction helper for the Platform-owned approved-deploy queue."""

import argparse
import hashlib
import json
import sqlite3
import sys
import time


def connect(path):
    db = sqlite3.connect(path, timeout=5, isolation_level=None)
    db.row_factory = sqlite3.Row
    db.execute("PRAGMA busy_timeout = 5000")
    return db


def decode_request(row):
    raw = row["request_json"]
    if hashlib.sha256(raw.encode("utf-8")).hexdigest() != row["body_hash"]:
        raise ValueError("stored deployment request digest mismatch")
    request = json.loads(raw)
    if (
        request.get("repository") != row["repository"]
        or request.get("branch") != row["branch"]
        or request.get("commit") != row["commit_hash"]
        or request.get("tree") != row["tree_hash"]
        or request.get("workflow_url") != row["workflow_url"]
        or request.get("summary") != row["summary"]
        or json.dumps(request.get("submodules"), separators=(",", ":"), ensure_ascii=False)
        != row["submodules_json"]
    ):
        raise ValueError("stored deployment request fields differ from immutable request body")
    return request


def claim(db):
    now = int(time.time())
    db.execute("BEGIN IMMEDIATE")
    try:
        db.execute(
            "UPDATE platform_deploy_requests SET status = 'expired', version = version + 1, decided_at_unix = ? "
            "WHERE status IN ('pending', 'approved') AND expires_at_unix <= ?",
            (now, now),
        )
        row = db.execute(
            "SELECT * FROM platform_deploy_requests WHERE status = 'approved' AND expires_at_unix > ? "
            "ORDER BY decided_at_unix, created_at_unix LIMIT 1",
            (now,),
        ).fetchone()
        if row is None:
            db.execute("COMMIT")
            print("null")
            return 0
        request = decode_request(row)
        changed = db.execute(
            "UPDATE platform_deploy_requests SET status = 'executing', version = version + 1 "
            "WHERE id = ? AND status = 'approved' AND version = ?",
            (row["id"], row["version"]),
        ).rowcount
        if changed != 1:
            raise ValueError("approved request changed before atomic claim")
        db.execute("COMMIT")
        print(json.dumps({"id": row["id"], "version": row["version"] + 1, "request": request}, separators=(",", ":"), ensure_ascii=False))
        return 0
    except Exception:
        db.execute("ROLLBACK")
        raise


def complete(db, request_id, version, result, detail):
    if result not in ("succeeded", "failed"):
        raise ValueError("invalid terminal result")
    now = int(time.time())
    db.execute("BEGIN IMMEDIATE")
    try:
        changed = db.execute(
            "UPDATE platform_deploy_requests SET status = ?, version = version + 1, "
            "decided_at_unix = ?, result_detail = ? "
            "WHERE id = ? AND status = 'executing' AND version = ?",
            (result, now, detail[:1000], request_id, version),
        ).rowcount
        if changed != 1:
            raise ValueError("deployment result does not match an executing request")
        db.execute("COMMIT")
    except Exception:
        db.execute("ROLLBACK")
        raise


def verify_claim(db, request_id, version, commit):
    row = db.execute(
        "SELECT * FROM platform_deploy_requests WHERE id = ? AND status = 'executing' AND version = ?",
        (request_id, version),
    ).fetchone()
    if row is None:
        raise ValueError("no matching executing deployment request")
    request = decode_request(row)
    if request["commit"] != commit:
        raise ValueError("executing request revision does not match the deployment argument")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--database", required=True)
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("claim")
    finish = subparsers.add_parser("complete")
    finish.add_argument("--id", required=True)
    finish.add_argument("--version", required=True, type=int)
    finish.add_argument("--result", required=True, choices=("succeeded", "failed"))
    finish.add_argument("--detail", default="")
    verify = subparsers.add_parser("verify")
    verify.add_argument("--id", required=True)
    verify.add_argument("--version", required=True, type=int)
    verify.add_argument("--commit", required=True)
    args = parser.parse_args()

    try:
        with connect(args.database) as db:
            if args.command == "claim":
                return claim(db)
            if args.command == "verify":
                verify_claim(db, args.id, args.version, args.commit)
            else:
                complete(db, args.id, args.version, args.result, args.detail)
            return 0
    except Exception as error:
        print(f"deploy-request-queue: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
