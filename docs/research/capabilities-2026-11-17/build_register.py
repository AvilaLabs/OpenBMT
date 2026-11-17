#!/usr/bin/env python3
"""Validate and assemble the research snapshot. Does not touch product code."""
from __future__ import annotations

import hashlib
import json
import re
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent
PROJECT = ROOT.parents[2]


def read(path):
    return json.loads(path.read_text())


def write(path, data):
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")


def unique(values):
    return list(dict.fromkeys(v for v in values if v))


def text(value):
    if isinstance(value, list):
        return "; ".join(text(v) for v in value)
    if isinstance(value, dict):
        return json.dumps(value, ensure_ascii=False)
    return str(value or "")


def md(value):
    return text(value).replace("|", "\\|").replace("\n", " ")


def main():
    contract = read(ROOT / "research-contract.json")
    bundles = [(p, read(p)) for p in sorted((ROOT / "vendors").glob("*.json"))]
    bundles.append((ROOT / "analysis/bitcoin-native.json", read(ROOT / "analysis/bitcoin-native.json")))
    errors = []
    all_ids = set()
    claims, sources, coverage, vendors = [], [], [], []
    for path, bundle in bundles:
        label = str(path.relative_to(ROOT))
        for field in contract["bundle_required"]:
            if field not in bundle:
                errors.append(f"{label}: missing bundle field {field}")
        sid = {s["id"] for s in bundle["sources"]}
        cid = {c["id"] for c in bundle["claims"]}
        kind = ("bitcoin_native_requirements" if path.name == "bitcoin-native.json" else
                "card_components" if path.name == "card-components.json" else "full_pos_benchmark")
        context = {"vendor": bundle["vendor"], "segment": bundle["segment"],
                   "benchmark_kind": kind, "bundle_path": label}
        for section, required in [("sources", "source_required"), ("claims", "claim_required"),
                                  ("coverage", "coverage_required")]:
            for row in bundle[section]:
                rid = row.get("id", "<missing id>")
                if rid in all_ids:
                    errors.append(f"Duplicate record ID: {rid}")
                all_ids.add(rid)
                for field in contract[required]:
                    if field not in row:
                        errors.append(f"{rid}: missing {field}")
                for ref in row.get("source_ids", []):
                    if ref not in sid:
                        errors.append(f"{rid}: unknown source {ref}")
                for ref in row.get("claim_ids", []):
                    if ref not in cid:
                        errors.append(f"{rid}: unknown claim {ref}")
                if section == "claims":
                    for field, allowed in [("category", "categories"), ("documented_status", "documented_status"),
                                           ("bitcoin_classification", "bitcoin_classification"), ("evidence_access", "evidence_access")]:
                        if row.get(field) not in contract[allowed]:
                            errors.append(f"{rid}: invalid {field} {row.get(field)}")
                    for field in ["improvement_hypothesis", "bitcoin_hypothesis", "bitcoin_tradeoffs", "acceptance_test"]:
                        if not text(row.get(field)).strip():
                            errors.append(f"{rid}: empty {field}")
                if section == "coverage" and row.get("status") not in contract["coverage_status"]:
                    errors.append(f"{rid}: invalid coverage status {row.get('status')}")
                {"sources": sources, "claims": claims, "coverage": coverage}[section].append({**row, **context})
        counts = Counter(c["status"] for c in bundle["coverage"])
        vendors.append({**context, "scope": bundle["scope"], "limitations": bundle["limitations"],
                        "claims": len(cid), "sources": len(sid), **{s: counts[s] for s in contract["coverage_status"]}})

    by_claim = {c["id"]: c for c in claims}
    evidence = read(ROOT / "analysis/openbmt-evidence.json")
    eid = {e["id"] for e in evidence["entries"]}
    mapping_path = ROOT / "analysis/openbmt-mapping.json"
    mappings = read(mapping_path).get("mappings", []) if mapping_path.exists() else []
    by_mapping = {}
    for row in mappings:
        if row["claim_id"] not in by_claim:
            errors.append(f"Mapping refers to unknown claim {row['claim_id']}")
        if row["claim_id"] in by_mapping:
            errors.append(f"Duplicate mapping for {row['claim_id']}")
        for ref in row["evidence_ids"]:
            if ref not in eid:
                errors.append(f"Mapping refers to unknown evidence {ref}")
        by_mapping[row["claim_id"]] = row

    normalization = read(ROOT / "analysis/normalization-decisions.json")
    override = {}
    for group in normalization["approved_alias_groups"]:
        for ref in group["claim_ids"]:
            if ref not in by_claim:
                errors.append(f"Normalization refers to unknown claim {ref}")
            if ref in override and override[ref] != group["canonical_key"]:
                errors.append(f"Conflicting normalized keys for {ref}")
            override[ref] = group["canonical_key"]
    relationships = []
    families = defaultdict(list)
    for path in sorted((ROOT / "analysis").glob("normalization-*.json")):
        if path.name == "normalization-decisions.json":
            continue
        review = read(path)
        for group in review.get("alias_groups", []) + review.get("family_groups", []):
            relationships.append({**group, "review_path": str(path.relative_to(ROOT)),
                                  "relationship": "related_merchant_job_not_proof_of_equivalence"})
            for ref in group["claim_ids"]:
                if ref not in by_claim:
                    errors.append(f"Normalization review refers to unknown claim {ref}")
                families[ref].append(group.get("canonical_key", group.get("family_key")))

    decisions = read(ROOT / "analysis/proposed-decisions.json")["decisions"]
    opportunities = read(ROOT / "analysis/bitcoin-opportunities.json")["opportunities"]
    raw_keys = {c["canonical_key"] for c in claims}
    for row in decisions + opportunities:
        for ref in row.get("affected_claim_ids", []) + row.get("claim_ids", []):
            if ref not in by_claim:
                errors.append(f"{row['id']}: unknown claim {ref}")
        for key in row.get("affected_keys", []) + row.get("capability_keys", []):
            if key not in raw_keys and key not in {g['canonical_key'] for g in normalization['approved_alias_groups']}:
                errors.append(f"{row['id']}: unknown capability key {key}")

    crosswalk = read(ROOT / "analysis/prior-benchmark-crosswalk.json")
    if {r['id'] for r in crosswalk['items']} != {f"B{i:02d}" for i in range(1, 33)}:
        errors.append("Prior benchmark crosswalk must preserve B01 through B32")
    for row in crosswalk['items']:
        for ref in row['current_claim_ids']:
            if ref not in by_claim:
                errors.append(f"{row['id']}: unknown crosswalk claim {ref}")
    if set(by_mapping) != set(by_claim):
        errors.append("Current-evidence mapping must contain exactly one record for every current claim")

    groups = defaultdict(list)
    for claim in claims:
        key = override.get(claim["id"], claim["canonical_key"])
        claim["normalized_key"] = key
        claim["related_family_keys"] = unique(families[claim["id"]])
        claim["openbmt_mapping"] = by_mapping.get(claim["id"], {
            "status": "not_assessed", "evidence_ids": [],
            "mapping_rationale": "No relevant implementation evidence mapped in this snapshot. This does not establish absence.",
            "unmet_scope": "Full benchmark behavior remains to be assessed and demonstrated."})
        groups[key].append(claim)

    capabilities = []
    for key, variants in sorted(groups.items(), key=lambda kv: (kv[1][0]["category"], kv[0])):
        cap_id = "CAP-" + hashlib.sha256(key.encode()).hexdigest()[:10].upper()
        kinds = unique(c["benchmark_kind"] for c in variants)
        target = ("evaluate_component" if kinds == ["card_components"] else
                  "prove_native_safety_requirement" if kinds == ["bitcoin_native_requirements"] else "match_or_improve_merchant_job")
        refs = [c["id"] for c in variants]
        relevant_decisions = [d["id"] for d in decisions if set(d.get("affected_claim_ids", [])) & set(refs)]
        mapped_evidence = unique(e for c in variants for e in c["openbmt_mapping"]["evidence_ids"])
        cap = {
            "id": cap_id, "canonical_key": key, "category": variants[0]["category"],
            "capability": variants[0]["capability"],
            "merchant_outcome": variants[0]["merchant_outcome"],
            "vendors": unique(c["vendor"] for c in variants), "benchmark_kinds": kinds,
            "benchmark_claim_ids": refs, "source_ids": unique(s for c in variants for s in c["source_ids"]),
            "proposed_disposition": target, "decision_ids": relevant_decisions,
            "current_status": "partial_implementation_evidence" if any(c['openbmt_mapping']['status'] != 'not_assessed' for c in variants) else "not_assessed",
            "parity_status": "unverified",
            "current_evidence_ids": mapped_evidence,
            "unmet_scope": [f"{c['id']}: {text(c['openbmt_mapping'].get('unmet_scope'))}" for c in variants],
            "improvement_hypotheses": [f"{c['id']}: {text(c['improvement_hypothesis'])}" for c in variants],
            "bitcoin_classifications": unique(c["bitcoin_classification"] for c in variants),
            "bitcoin_hypotheses": [f"{c['id']}: {text(c['bitcoin_hypothesis'])}" for c in variants],
            "bitcoin_tradeoffs": [f"{c['id']}: {text(c['bitcoin_tradeoffs'])}" for c in variants],
            "acceptance_tests": [f"{c['id']}: {text(c['acceptance_test'])}" for c in variants],
            "related_family_keys": unique(f for c in variants for f in c["related_family_keys"]),
            "comparison_note": "Every vendor variant and its qualifiers remains an obligation to assess. Group membership does not establish identical behavior, implementation, compatibility or parity."
        }
        capabilities.append(cap)
        for claim in variants:
            claim["capability_id"] = cap_id

    totals = {"capabilities": len(capabilities), "claims": len(claims), "sources": len(sources),
              "coverage": len(coverage), "decisions": len(decisions), "opportunities": len(opportunities)}
    stats = {}
    for kind in ["full_pos_benchmark", "card_components", "bitcoin_native_requirements"]:
        subset = [c for c in claims if c["benchmark_kind"] == kind]
        cov = [c for c in coverage if c["benchmark_kind"] == kind]
        stats[kind] = {"claims": len(subset), "source_records": sum(s["benchmark_kind"] == kind for s in sources),
                       "coverage_entries": len(cov), "coverage_status": dict(Counter(c["status"] for c in cov)),
                       "claim_documentation_status": dict(Counter(c["documented_status"] for c in subset))}
    result = {"metadata": {
        "schema": "openbmt-capability-register-v1", "title": "OpenBMT capability review", "as_of": contract["as_of"],
        "source_commit": evidence["source_commit"], "counts": totals, "populations": stats,
        "scope_note": "Research only. Four bounded first-party product corpora, separate card components, and Bitcoin-native requirements. No whole-product exhaustiveness, tested compatibility, feature parity, product approval or release readiness is claimed.",
        "default_rule": "Match or improve every retained merchant job and documented variant unless an explicit mechanism exception or scope deferral is approved. A deferral remains an open requirement.",
        "normalization_policy": "Only coordinator-approved narrow aliases are combined. Related families aid navigation and never erase variant requirements.",
        "status_policy": "Related evidence is not parity. No mapping means not assessed, never absent. Engine primitives, sandbox flows and scoped tests do not establish a complete merchant workflow.",
        "source_count_note": "Source records are exact page/section inspections; repeated URLs across sections or researchers count separately. Coverage rows are topics, not an equal-weight percentage of a product.",
        "workflow": "Research/planning complete as a bounded snapshot; unresolved branches and production hypotheses remain explicit gates. No application implementation in this phase."
    }, "capabilities": capabilities, "vendors": vendors, "claims": claims, "sources": sources,
       "coverage": coverage, "decisions": decisions, "opportunities": opportunities,
       "normalization_relationships": relationships, "openbmt_evidence": evidence}
    if errors:
        raise SystemExit("Research validation failed:\n" + "\n".join(errors))
    write(ROOT / "capability-register.json", result)
    report = {"as_of": contract["as_of"], "source_commit": evidence["source_commit"], "schema_and_references": "passed",
              "counts": totals, "populations": stats, "unique_source_urls": len({s['url'] for s in sources}),
              "records_with_positive_implementation_evidence": sum(c['current_status'] == 'partial_implementation_evidence' for c in capabilities),
              "records_with_related_or_boundary_evidence": sum(bool(c['current_evidence_ids']) for c in capabilities),
              "unverified_parity_records": len(capabilities), "application_builds_or_tests": "not run: research-only changes"}
    report['input_sha256'] = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(
        [p for p, _ in bundles] + [ROOT / 'analysis/openbmt-evidence.json', ROOT / 'analysis/openbmt-mapping.json',
                                   ROOT / 'analysis/proposed-decisions.json', ROOT / 'analysis/bitcoin-opportunities.json',
                                   ROOT / 'analysis/prior-benchmark-crosswalk.json', ROOT / 'analysis/normalization-decisions.json'])}
    write(ROOT / "analysis/validation.json", report)

    index = ["# Capability index", "", "Generated from the validated register. IDs remain stable when rows are reordered. Full vendor behaviors, qualifications, hypotheses and acceptance scenarios are in [the register](capability-register.json) and the review workbook.", "", "Each row is a comparison record. All referenced claim variants remain visible and required to assess. No row is counted as matched or surpassed.", ""]
    for category in sorted({c["category"] for c in capabilities}):
        index += [f"## {category.replace('_', ' ').title()}", "", "| ID | Capability | Benchmark claims | Current evidence | Proposed handling |", "| --- | --- | --- | --- | --- |"]
        for cap in capabilities:
            if cap["category"] == category:
                index.append(f"| {cap['id']} | {md(cap['capability'])} | {', '.join(cap['benchmark_claim_ids'])} | {', '.join(cap['current_evidence_ids']) or 'Not assessed'} | {cap['proposed_disposition'].replace('_', ' ')} |")
        index.append("")
    (ROOT / "capability-index.md").write_text("\n".join(index).rstrip() + "\n")
    lines = ["# Source coverage", "", "A reviewed row means the named evidence boundary was inspected. It does not mean the vendor feature or OpenBMT implementation was tested, or that every leaf beneath a linked index was covered. Exclusions retain their reasons. Unresolved rows remain open research work. Counts are not product-completeness percentages.", ""]
    for vendor in vendors:
        lines += [f"## {vendor['vendor']} — {Path(vendor['bundle_path']).stem}", "", md(vendor['scope']), "", "| ID | Topic | Status | Evidence boundary or reason | Sources |", "| --- | --- | --- | --- | --- |"]
        for row in coverage:
            if row['bundle_path'] == vendor['bundle_path']:
                lines.append(f"| {row['id']} | [{md(row['topic'])}]({row['url']}) | {row['status']} | {md(row['reason'])} | {', '.join(row['source_ids'])} |")
        lines += ["", "Limitations: " + md(vendor['limitations']), ""]
    (ROOT / "source-coverage.md").write_text("\n".join(lines).rstrip() + "\n")
    review = ["# Bitcoin opportunities and proposed decisions", "", "Design hypotheses from the capability research. These are review proposals and validation targets, not implemented features or verified outcomes. Exact benchmark conditions remain in the [register](capability-register.json).", ""]
    for row in opportunities:
        review += [f"## {row['id']}: {row['title']}", "", f"**Merchant job:** {row['merchant_job']}", "",
                   f"**What Bitcoin could improve:** {row['what_bitcoin_improves']}", "",
                   f"**What it does not solve:** {row['what_bitcoin_does_not_improve']}", "",
                   f"**Candidate:** {row['architecture_candidate']}", "", f"**Costs and tradeoffs:** {row['tradeoffs']}", "",
                   f"**Proof needed:** {row['validation_needed']}", "", f"Benchmark references: {', '.join(row['claim_ids'])}.", ""]
    review += ["## Proposed decisions", "", "Established principles, current code limits, and proposed design choices are explicitly distinguished below. Changing a mechanism does not reject the merchant job. Deferral does not close a requirement.", ""]
    for row in decisions:
        review += [f"### {row['id']}: {row['title']}", "", f"**Proposal:** {row['proposal']}", "",
                   f"**Preserved job:** {row['preserved_merchant_job']}", "", f"**Reason:** {row['rationale']}", "",
                   f"**Basis:** {row['user_authorization_status']}", "", f"**Proof or revisit trigger:** {row['proof_or_revisit_trigger']}", ""]
    (ROOT / 'bitcoin-review.md').write_text('\n'.join(review).rstrip() + '\n')

    counts_table = ["| Population | Source claims | Source records | Coverage topics | Reviewed / excluded / unresolved |",
                    "| --- | ---: | ---: | ---: | --- |"]
    for kind, label in [('full_pos_benchmark', 'Four product corpora'), ('card_components', 'Card components'),
                        ('bitcoin_native_requirements', 'Bitcoin-native requirements')]:
        s = stats[kind]; c = s['coverage_status']
        counts_table.append(f"| {label} | {s['claims']} | {s['source_records']} | {s['coverage_entries']} | {c.get('reviewed', 0)} / {c.get('excluded', 0)} / {c.get('unresolved', 0)} |")
    counts_table += ['', f"The consolidated register contains **{len(capabilities)} comparison records**, retaining **{len(claims)} source claims**, plus **{len(opportunities)} Bitcoin opportunity proposals** and **{len(decisions)} decision/gate proposals**. Source records cover {report['unique_source_urls']} distinct URLs; repeated page sections count separately. The prior crosswalk preserves all 32 benchmark families.", '',
                     "A source claim is not a unique feature count. A comparison record may preserve multiple qualified vendor variants. These totals describe the research artifact, not a percentage of product coverage or OpenBMT completion."]
    readme = ROOT / 'README.md'
    readme.write_text(re.sub(r'<!-- SNAPSHOT_COUNTS_START -->.*?<!-- SNAPSHOT_COUNTS_END -->',
                            '<!-- SNAPSHOT_COUNTS_START -->\n' + '\n'.join(counts_table) + '\n<!-- SNAPSHOT_COUNTS_END -->',
                            readme.read_text(), flags=re.S))
    print(json.dumps({k: v for k, v in report.items() if k != 'input_sha256'}, indent=2))


if __name__ == "__main__":
    main()
