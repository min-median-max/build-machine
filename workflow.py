#!/usr/bin/env python3
"""Small, deliberately strict GitHub Actions workflow reader.

The build machine does not attempt to implement every GitHub Actions feature.
It reads the repository workflow as the contract and fails validation when a
step needs an adapter that has not been implemented here.
"""
from __future__ import annotations

import ast
import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any


class WorkflowError(ValueError):
    pass


SUPPORTED_USES = {
    "actions/checkout": "checkout",
    "pnpm/action-setup": "pnpm-setup",
    "actions/setup-node": "node-setup",
    "dtolnay/rust-toolchain": "rust-setup",
    "swatinem/rust-cache": "cache",
    "actions/cache": "cache",
    "tauri-apps/tauri-action": "tauri-build",
    "actions/upload-artifact": "artifact-upload",
    "actions/upload-pages-artifact": "artifact-upload",
    "softprops/action-gh-release": "release",
}


def _strip_comment(line: str) -> str:
    quote = None
    escaped = False
    for index, char in enumerate(line):
        if escaped:
            escaped = False
            continue
        if char == "\\" and quote == '"':
            escaped = True
            continue
        if char in "'\"":
            quote = None if quote == char else (char if quote is None else quote)
        elif char == "#" and quote is None and (index == 0 or line[index - 1].isspace()):
            return line[:index].rstrip()
    return line.rstrip()


def _scalar(value: str) -> Any:
    value = value.strip()
    if not value:
        return None
    if value in ("|", ">"):
        return value
    if (value.startswith("'") and value.endswith("'")) or (value.startswith('"') and value.endswith('"')):
        try:
            return ast.literal_eval(value)
        except (SyntaxError, ValueError):
            return value[1:-1]
    if value.startswith("[") and value.endswith("]"):
        try:
            return ast.literal_eval(value.replace("true", "True").replace("false", "False"))
        except (SyntaxError, ValueError):
            return [item.strip().strip("'\"") for item in value[1:-1].split(",") if item.strip()]
    if value in ("true", "false"):
        return value == "true"
    if value in ("null", "~"):
        return None
    if re.fullmatch(r"-?\d+", value):
        return int(value)
    return value


def _key_value(text: str) -> tuple[str, str] | None:
    match = re.match(r"([^:]+):(?:\s*(.*))?$", text)
    if not match:
        return None
    return match.group(1).strip().strip("'\""), match.group(2) or ""


def _parse_yaml(text: str) -> dict[str, Any]:
    """Parse the subset used by repository workflows without a new dependency."""
    raw = text.splitlines()
    lines: list[tuple[int, str, str]] = []
    comments: list[str] = []
    for number, original in enumerate(raw, 1):
        if "build-machine:" in original:
            comments.append(original.strip())
        if not original.strip() or original.lstrip().startswith("#"):
            continue
        indent = len(original) - len(original.lstrip(" "))
        cleaned = _strip_comment(original[indent:])
        if cleaned:
            lines.append((indent, cleaned, original))

    def parse_block(position: int, indent: int) -> tuple[Any, int]:
        if position >= len(lines) or lines[position][0] < indent:
            return {}, position
        is_list = lines[position][0] == indent and lines[position][1].startswith("-")
        result: Any = [] if is_list else {}
        while position < len(lines):
            current_indent, text_value, original = lines[position]
            if current_indent < indent:
                break
            if current_indent > indent:
                raise WorkflowError(f"지원하지 않는 YAML 들여쓰기({position + 1}행): {original.strip()}")
            if is_list:
                if not text_value.startswith("-"):
                    break
                item_text = text_value[1:].strip()
                position += 1
                if not item_text:
                    if position < len(lines) and lines[position][0] > indent:
                        item, position = parse_block(position, lines[position][0])
                    else:
                        item = None
                else:
                    pair = _key_value(item_text)
                    if pair is None:
                        item = _scalar(item_text)
                    else:
                        key, value = pair
                        item = {key: _scalar(value)}
                        if value in ("|", ">"):
                            item[key], position = parse_literal(position, indent + 2, folded=value == ">")
                        if position < len(lines) and lines[position][0] > indent:
                            extra, position = parse_block(position, lines[position][0])
                            if isinstance(extra, dict):
                                item.update(extra)
                result.append(item)
            else:
                pair = _key_value(text_value)
                if pair is None:
                    raise WorkflowError(f"워크플로 키를 읽지 못했어요({position + 1}행): {original.strip()}")
                key, value = pair
                position += 1
                if value in ("|", ">"):
                    parsed, position = parse_literal(position, indent + 2, folded=value == ">")
                elif value:
                    parsed = _scalar(value)
                elif position < len(lines) and lines[position][0] > indent:
                    parsed, position = parse_block(position, lines[position][0])
                else:
                    parsed = {}
                result[key] = parsed
        return result, position

    def parse_literal(position: int, child_indent: int, folded: bool = False) -> tuple[str, int]:
        values: list[str] = []
        while position < len(lines) and lines[position][0] >= child_indent:
            current_indent, text_value, _ = lines[position]
            values.append(" " * max(0, current_indent - child_indent) + text_value)
            position += 1
        return ((" " if folded else "\n").join(values) + "\n"), position

    parsed, position = parse_block(0, lines[0][0] if lines else 0)
    if position != len(lines) or not isinstance(parsed, dict):
        raise WorkflowError("워크플로 최상위 구조가 올바르지 않아요.")
    parsed["__build_machine_comments"] = comments
    return parsed


def _uses_key(value: str) -> tuple[str, str]:
    if "@" not in value:
        raise WorkflowError(f"액션 ref가 없는 uses 단계는 지원하지 않아요: {value}")
    name, ref = value.split("@", 1)
    return name, ref


def _steps(job: dict[str, Any]) -> list[dict[str, Any]]:
    values = job.get("steps")
    if not isinstance(values, list) or not values:
        raise WorkflowError("각 job에는 하나 이상의 steps가 필요해요.")
    result = []
    for index, value in enumerate(values, 1):
        if not isinstance(value, dict):
            raise WorkflowError(f"step {index}가 객체가 아니에요.")
        if "uses" not in value and "run" not in value:
            raise WorkflowError(f"step {index}에는 uses 또는 run이 필요해요.")
        if "uses" in value:
            name, ref = _uses_key(str(value["uses"]))
            adapter = SUPPORTED_USES.get(name)
            if not adapter:
                raise WorkflowError(f"어댑터가 없는 GitHub action은 실행할 수 없어요: {name}@{ref}")
            value = dict(value, action=name, ref=ref, adapter=adapter)
        else:
            value = dict(value, action="run", adapter="run")
        value["index"] = index
        value.setdefault("name", value.get("uses") or "run")
        result.append(value)
    return result


def _skip_comments(comments: list[str]) -> dict[str, str]:
    skips: dict[str, str] = {}
    for comment in comments:
        match = re.search(r"build-machine:\s*skip\s+(test|smoke)\s+reason=(.+)$", comment)
        if match:
            reason = match.group(2).strip()
            if reason:
                skips[match.group(1)] = reason
    return skips


ADAPTER_STAGES = {
    "checkout": "setup",
    "pnpm-setup": "setup",
    "node-setup": "setup",
    "rust-setup": "setup",
    "cache": "setup",
    "tauri-build": "build",
    "artifact-upload": "release",
    "release": "release",
}


def _stage_for(step: dict[str, Any]) -> str:
    """Classify a step by its adapter; only shell steps use the name heuristic.

    An action's own name must never satisfy a stage gate. `actions/checkout`
    contains "check" and would otherwise be read as a test stage, letting a
    workflow with no test command pass validation without the explicit
    `# build-machine: skip test reason=...` comment.
    """
    adapter = step.get("adapter")
    if adapter in ADAPTER_STAGES:
        return ADAPTER_STAGES[adapter]
    text = " ".join(str(step.get(key, "")) for key in ("name", "run")).lower()
    if any(word in text for word in ("smoke", "launch", "health", "e2e")):
        return "smoke"
    if any(word in text for word in ("test", "lint", "check", "verify")):
        return "test"
    if any(word in text for word in ("build", "package", "compile")):
        return "build"
    return "setup"


@dataclass(frozen=True)
class Workflow:
    path: str
    name: str
    jobs: tuple[dict[str, Any], ...]
    event: str
    ref: str | None
    skips: dict[str, str]
    raw: dict[str, Any]

    def serializable(self) -> dict[str, Any]:
        return {"path": self.path, "name": self.name, "jobs": list(self.jobs), "event": self.event,
                "ref": self.ref, "skips": self.skips}


def load(path: Path, event: str = "workflow_dispatch", ref: str | None = None) -> Workflow:
    if not path.is_file():
        raise WorkflowError(f"워크플로 파일을 찾을 수 없어요: {path}")
    raw = _parse_yaml(path.read_text())
    triggers = raw.get("on")
    if triggers is None:
        raise WorkflowError("워크플로에 이벤트(on)가 없어요.")
    if isinstance(triggers, str):
        available = {triggers}
    elif isinstance(triggers, list):
        available = set(map(str, triggers))
    elif isinstance(triggers, dict):
        available = set(map(str, triggers))
    else:
        available = set()
    if event not in available:
        raise WorkflowError(f"이 워크플로는 {event} 이벤트를 지원하지 않아요. 지원 이벤트: {', '.join(sorted(available))}")
    jobs_value = raw.get("jobs")
    if not isinstance(jobs_value, dict) or not jobs_value:
        raise WorkflowError("워크플로에 jobs가 없어요.")
    jobs = []
    for job_id, job in jobs_value.items():
        if not isinstance(job, dict):
            raise WorkflowError(f"job {job_id}가 객체가 아니에요.")
        if not job.get("runs-on"):
            raise WorkflowError(f"job {job_id}에 runs-on이 없어요.")
        if any(key in job for key in ("container", "services", "uses", "strategy")):
            raise WorkflowError(f"job {job_id}의 container/services/reusable/matrix는 아직 지원하지 않아요.")
        normalized = dict(job, id=str(job_id), steps=_steps(job))
        if normalized.get("needs") and not isinstance(normalized["needs"], (str, list)):
            raise WorkflowError(f"job {job_id}의 needs 형식이 올바르지 않아요.")
        jobs.append(normalized)
    known = {job["id"] for job in jobs}
    for job in jobs:
        needs = job.get("needs", [])
        needs = [needs] if isinstance(needs, str) else needs
        missing = [str(item) for item in needs if str(item) not in known]
        if missing:
            raise WorkflowError(f"job {job['id']}가 없는 needs를 참조해요: {', '.join(missing)}")
    ordered = []
    pending = list(jobs)
    while pending:
        ready = [job for job in pending if all(str(dep) in {item["id"] for item in ordered} for dep in ([job.get("needs")] if isinstance(job.get("needs"), str) else job.get("needs", [])))]
        if not ready:
            raise WorkflowError("job needs 순환 또는 실행 순서를 확인할 수 없어요.")
        ordered.extend(ready)
        pending = [job for job in pending if job not in ready]
    jobs = ordered
    skips = _skip_comments(raw.get("__build_machine_comments", []))
    stages = {_stage_for(step) for job in jobs for step in job["steps"]}
    if "build" not in stages:
        raise WorkflowError("build 단계가 없어요. 지원하는 build action 또는 build 명령을 workflow에 추가해야 해요.")
    for required in ("test", "smoke"):
        if required not in stages and required not in skips:
            raise WorkflowError(f"{required} 단계가 없어요. 워크플로에 '# build-machine: skip {required} reason=...' 주석을 추가해야 해요.")
    return Workflow(str(path), str(raw.get("name") or path.stem), tuple(jobs), event, ref, skips, raw)


def discover(root: Path, requested: str | None = None, event: str = "workflow_dispatch", ref: str | None = None) -> Workflow:
    if requested:
        path = Path(requested)
        if not path.is_absolute():
            path = root / path
        return load(path.resolve(), event, ref)
    files = sorted((root / ".github" / "workflows").glob("*.y*ml"))
    if len(files) != 1:
        if not files:
            raise WorkflowError(".github/workflows에 워크플로가 없어요.")
        raise WorkflowError("워크플로가 여러 개예요. --workflow로 하나를 선택해야 해요.")
    return load(files[0], event, ref)


def stage_steps(workflow: Workflow) -> dict[str, list[dict[str, Any]]]:
    stages: dict[str, list[dict[str, Any]]] = {"setup": [], "test": [], "build": [], "smoke": [], "release": []}
    for job in workflow.jobs:
        for step in job["steps"]:
            step = dict(step, jobId=job["id"], jobEnv=job.get("env") or {})
            stages[_stage_for(step)].append(step)
    for stage in ("test", "smoke"):
        if not stages[stage] and stage in workflow.skips:
            stages[stage] = [{"name": f"skip {stage}", "adapter": "skip", "reason": workflow.skips[stage], "index": 0}]
    return stages


def as_json(workflow: Workflow) -> str:
    return json.dumps(workflow.serializable(), indent=2, ensure_ascii=False)
