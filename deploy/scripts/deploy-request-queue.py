#!/usr/bin/env python3
"""Small transaction helper for the Platform-owned approved-deploy queue."""

import argparse
import hashlib
import json
import os
import pathlib
import re
import sqlite3
import subprocess
import sys
import time
import uuid

SERVICE_TARGETS = {
    "atlas",
    "homepage",
    "docs",
    "identity",
    "diagnostics",
    "registry",
    "artifact",
    "build-cache",
}
DISPATCH_STALE_SECONDS = 30


def validate_selection(request):
    server_mode = request.get("server_mode")
    scope = request.get("deployment_scope")
    target = request.get("service_target")
    if server_mode not in ("monolith", "services"):
        raise ValueError("request has no valid deployment topology")
    if scope == "full" and target is None:
        return
    if scope != "service":
        raise ValueError("request has an invalid deployment scope/target combination")
    if server_mode != "services":
        raise ValueError("service deployment scope requires services topology")
    if target not in SERVICE_TARGETS:
        raise ValueError("service deployment scope has an invalid service target")


def connect(path):
    database_uri = pathlib.Path(path).resolve().as_uri() + "?mode=rw"
    db = sqlite3.connect(database_uri, uri=True, timeout=5, isolation_level=None)
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


def claim(db, request_id):
    now = int(time.time())
    db.execute("BEGIN IMMEDIATE")
    try:
        db.execute(
            "UPDATE platform_deploy_requests SET status = 'expired', version = version + 1, decided_at_unix = ?, "
            "dispatch_state = CASE "
            "WHEN status = 'approved' AND dispatch_state IN ('queued', 'dispatching') THEN 'expired' "
            "ELSE dispatch_state END, "
            "dispatch_detail = CASE "
            "WHEN status = 'approved' AND dispatch_state IN ('queued', 'dispatching') "
            "THEN 'Approval expired before the worker claimed this request.' "
            "ELSE dispatch_detail END "
            "WHERE status IN ('pending', 'approved') AND expires_at_unix <= ?",
            (now, now),
        )
        row = db.execute(
            "SELECT * FROM platform_deploy_requests WHERE id = ? AND status = 'approved' "
            "AND expires_at_unix > ? AND dispatch_state IN ('dispatching', 'queued') "
            "AND dispatch_attempted_at_unix > ?",
            (request_id, now, now - DISPATCH_STALE_SECONDS),
        ).fetchone()
        if row is None:
            db.execute("COMMIT")
            print("null")
            return 0
        request = decode_request(row)
        try:
            validate_selection(request)
        except ValueError as error:
            detail = f"Approved request has invalid deployment scope or target ({error}); no deployment was run. Submit a fresh CI request."
            db.execute(
                "UPDATE platform_deploy_requests SET status = 'failed', version = version + 1, "
                "decided_at_unix = ?, dispatch_state = 'failed', dispatch_detail = ?, result_detail = ? "
                "WHERE id = ? AND status = 'approved' AND version = ?",
                (now, detail, detail, row["id"], row["version"]),
            )
            db.execute("COMMIT")
            print("null")
            return 0
        changed = db.execute(
            "UPDATE platform_deploy_requests SET status = 'executing', version = version + 1, "
            "dispatch_state = 'claimed', dispatch_detail = NULL "
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
            "decided_at_unix = ?, result_detail = ?, dispatch_state = 'completed', dispatch_detail = NULL "
            "WHERE id = ? AND status = 'executing' AND version = ?",
            (result, now, detail[:1000], request_id, version),
        ).rowcount
        if changed != 1:
            raise ValueError("deployment result does not match an executing request")
        db.execute("COMMIT")
    except Exception:
        db.execute("ROLLBACK")
        raise


def approve_and_queue(db, request_id, expected_commit, reason, now=None):
    try:
        canonical_id = str(uuid.UUID(request_id))
    except (ValueError, AttributeError):
        raise ValueError("request ID must be a canonical UUID") from None
    if canonical_id != request_id:
        raise ValueError("request ID must be a canonical UUID")
    if not isinstance(expected_commit, str) or not re.fullmatch(r"[0-9a-fA-F]{40}", expected_commit):
        raise ValueError("expected commit must be a full 40-character SHA")
    if not isinstance(reason, str) or not 4 <= len(reason.strip()) <= 200:
        raise ValueError("approval reason must contain 4 to 200 characters")
    if any(ord(character) < 32 or ord(character) == 127 for character in reason):
        raise ValueError("approval reason cannot contain control characters")

    now = int(time.time()) if now is None else int(now)
    decision_reason = f"Local root operator approval: {reason.strip()}"
    db.execute("BEGIN IMMEDIATE")
    try:
        row = db.execute(
            "SELECT * FROM platform_deploy_requests WHERE id = ?", (canonical_id,)
        ).fetchone()
        if row is None:
            raise ValueError("deployment request was not found")
        if row["status"] != "pending":
            raise ValueError("only a pending deployment request can be approved")
        if int(row["expires_at_unix"]) <= now:
            raise ValueError("deployment request has expired")
        request = decode_request(row)
        validate_selection(request)
        if row["repository"] != "GHF-Studios/Vapor-Platform-Server":
            raise ValueError("only the official Platform repository can be approved")
        if row["branch"] != "main" or request.get("branch") != "main":
            raise ValueError("only the protected main branch can be approved")
        if row["commit_hash"] != expected_commit.lower() or request.get("commit") != expected_commit.lower():
            raise ValueError("expected commit does not match the immutable request")
        if not row["workflow_url"].startswith(
            "https://github.com/GHF-Studios/Vapor-Platform-Server/actions/runs/"
        ):
            raise ValueError("request has no official Platform CI workflow")

        version = int(row["version"])
        changed = db.execute(
            "UPDATE platform_deploy_requests SET status='approved', version=version+1, "
            "decided_by_profile_id=NULL, decided_at_unix=?, decision_reason=?, "
            "dispatch_state='dispatching', dispatch_attempts=dispatch_attempts+1, "
            "dispatch_attempted_at_unix=?, dispatch_detail=NULL "
            "WHERE id=? AND status='pending' AND version=? AND expires_at_unix>?",
            (now, decision_reason, now, canonical_id, version, now),
        ).rowcount
        if changed != 1:
            raise ValueError("deployment request changed during approval")
        db.execute("COMMIT")
    except Exception:
        db.execute("ROLLBACK")
        raise

    # This command is called only by the root-only CLI entrypoint. It starts
    # the existing fixed request unit; no caller-supplied executable or args.
    try:
        result = subprocess.run(
            ["/usr/bin/systemctl", "start", "--no-block", f"vapor-deploy@{canonical_id}.service"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
            timeout=5,
        )
        started = result.returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        started = False

    if not started:
        db.execute("BEGIN IMMEDIATE")
        try:
            changed = db.execute(
                "UPDATE platform_deploy_requests SET dispatch_state='failed', "
                "dispatch_detail='Root approval was saved, but the fixed deploy worker could not be queued; retry is available.' "
                "WHERE id=? AND status='approved' AND version=? AND dispatch_state='dispatching'",
                (canonical_id, version + 1),
            ).rowcount
            if changed != 1:
                raise ValueError("approval was saved, but dispatch state changed unexpectedly")
            db.execute("COMMIT")
        except Exception:
            db.execute("ROLLBACK")
            raise
        raise RuntimeError("approval was recorded, but the fixed deploy worker could not be queued; retry is available")
    return 0


def verify_claim(db, request_id, version, commit, server_mode, deployment_scope, service_target):
    row = db.execute(
        "SELECT * FROM platform_deploy_requests WHERE id = ? AND status = 'executing' AND version = ?",
        (request_id, version),
    ).fetchone()
    if row is None:
        raise ValueError("no matching executing deployment request")
    request = decode_request(row)
    if request["commit"] != commit:
        raise ValueError("executing request revision does not match the deployment argument")
    if request.get("server_mode") != server_mode or server_mode not in ("monolith", "services"):
        raise ValueError("executing request topology does not match the deployment argument")
    validate_selection(request)
    if request.get("deployment_scope") != deployment_scope:
        raise ValueError("executing request scope does not match the deployment argument")
    if request.get("service_target") != service_target:
        raise ValueError("executing request target does not match the deployment argument")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--database", required=True)
    subparsers = parser.add_subparsers(dest="command", required=True)
    claim_parser = subparsers.add_parser("claim")
    claim_parser.add_argument("--id", required=True)
    finish = subparsers.add_parser("complete")
    finish.add_argument("--id", required=True)
    finish.add_argument("--version", required=True, type=int)
    finish.add_argument("--result", required=True, choices=("succeeded", "failed"))
    finish.add_argument("--detail", default="")
    approve = subparsers.add_parser("approve-and-queue")
    approve.add_argument("--id", required=True)
    approve.add_argument("--commit", required=True)
    approve.add_argument("--reason", required=True)
    verify = subparsers.add_parser("verify")
    verify.add_argument("--id", required=True)
    verify.add_argument("--version", required=True, type=int)
    verify.add_argument("--commit", required=True)
    verify.add_argument("--server-mode", required=True, choices=("monolith", "services"))
    verify.add_argument("--deployment-scope", required=True, choices=("full", "service"))
    verify.add_argument("--service-target", default="")
    args = parser.parse_args()

    if args.command == "approve-and-queue" and getattr(os, "geteuid", lambda: -1)() != 0:
        print("deploy-request-queue: approval requires root SSH access", file=sys.stderr)
        return 1

    try:
        with connect(args.database) as db:
            if args.command == "claim":
                return claim(db, args.id)
            if args.command == "approve-and-queue":
                result = approve_and_queue(db, args.id, args.commit, args.reason)
                print(json.dumps({"id": args.id, "status": "approved", "dispatch_state": "dispatching"}))
                return result
            if args.command == "verify":
                verify_claim(
                    db,
                    args.id,
                    args.version,
                    args.commit,
                    args.server_mode,
                    args.deployment_scope,
                    args.service_target or None,
                )
            else:
                complete(db, args.id, args.version, args.result, args.detail)
            return 0
    except Exception as error:
        print(f"deploy-request-queue: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
