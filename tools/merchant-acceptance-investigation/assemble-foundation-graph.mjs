#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const [laneDirectory, outputPath, selectionPath] = process.argv.slice(2);
if (!laneDirectory || !outputPath) {
  console.error("usage: assemble-foundation-graph.mjs <lane-directory> <output-path>");
  process.exit(2);
}

const repositoryRoot = process.cwd();
const executionPath = path.join(repositoryRoot, "docs/research/capability-execution-2026-11-17.json");
const auditPath = path.join(repositoryRoot, "docs/research/full-capability-audit-2026-11-17.json");
const execution = JSON.parse(fs.readFileSync(executionPath, "utf8"));
const audit = JSON.parse(fs.readFileSync(auditPath, "utf8"));
const executionById = new Map(execution.capabilities.map((row) => [row.id, row]));
const executionByKey = new Map(execution.capabilities.map((row) => [row.key, row]));
const auditById = new Map(audit.capabilities.map((row) => [row.capability_id, row]));
const expectedLaneFiles = [
  "business-customer-multiloc.json",
  "cash-reporting.json",
  "catalog-inventory.json",
  "checkout-a.json",
  "checkout-b.json",
  "hardware-integrations.json",
  "lightning-offline.json",
  "onchain-backup.json",
  "payments-online.json",
  "pricing-tax.json",
  "refunds-returns.json",
  "security-staff.json",
];

const layerAliases = new Map([
  ["L0", "L0_invariant"],
  ["L1", "L1_shared_foundation"],
  ["L2", "L2_core_workflow"],
  ["L3", "L3_enhancement"],
  ["L4", "L4_vertical_or_adapter"],
  ["L0_invariant", "L0_invariant"],
  ["L1_shared_foundation", "L1_shared_foundation"],
  ["L2_core_workflow", "L2_core_workflow"],
  ["L3_enhancement", "L3_enhancement"],
  ["L4_vertical_or_adapter", "L4_vertical_or_adapter"],
]);

function inferredSecurityLayer(row) {
  const foundationClass = row.foundation_class ?? "";
  const order = Number(row.foundation_order ?? 5);
  if (order <= 1) return "L0_invariant";
  if (order === 2) return "L1_shared_foundation";
  if (
    order === 3 &&
    (foundationClass.includes("foundation") ||
      foundationClass.includes("boundary") ||
      foundationClass.includes("key_custody"))
  ) return "L1_shared_foundation";
  if (order <= 4) return "L2_core_workflow";
  if (order === 5) return "L4_vertical_or_adapter";
  return "L3_enhancement";
}

function normalizeFoundationCandidate(row) {
  if (typeof row.foundation_candidate === "boolean") return row.foundation_candidate;
  if (row.foundation_candidate === "strong") return true;
  if (row.foundation_candidate === "conditional" || row.foundation_candidate === "later") return false;
  return new Set([
    "platform_foundation",
    "platform_privacy_foundation",
    "domain_security_foundation",
    "domain_key_custody",
    "platform_authority_boundary",
    "trust_boundary_requirement",
  ]).has(row.foundation_class ?? "");
}

function rowsFromArtifact(artifact) {
  if (Array.isArray(artifact)) {
    return artifact.filter(
      (row) => row && typeof row === "object" && (row.capability_id || row.id),
    );
  }
  if (Array.isArray(artifact.rows)) return artifact.rows;
  if (Array.isArray(artifact.capabilities)) return artifact.capabilities;
  return [];
}

function uniqueStrings(values) {
  return [...new Set(values.filter((value) => typeof value === "string" && value.length > 0))].sort();
}

function countBy(values) {
  const result = {};
  for (const value of values) result[value] = (result[value] ?? 0) + 1;
  return Object.fromEntries(Object.entries(result).sort(([a], [b]) => a.localeCompare(b)));
}

function normalizeEvidencePath(evidencePath) {
  const repositoryPrefix = `${path.basename(repositoryRoot)}/`;
  return evidencePath.startsWith(repositoryPrefix)
    ? evidencePath.slice(repositoryPrefix.length)
    : evidencePath;
}

function classifyExternalDependencies(keys) {
  const result = {
    release_gate_keys: [],
    synthetic_primitive_keys: [],
    adapter_gate_keys: [],
    external_contract_keys: [],
  };
  for (const key of uniqueStrings(keys)) {
    if (key.startsWith("GATE-")) result.release_gate_keys.push(key);
    else if (key.startsWith("FOUND-")) result.synthetic_primitive_keys.push(key);
    else if (key.startsWith("ADAPTER-")) result.adapter_gate_keys.push(key);
    else result.external_contract_keys.push(key);
  }
  return result;
}

const removedDependencyEdges = [
  ["CAP-F122453871", "CAP-CCA3A4522A", "Backup creation and deployment recovery are peers in one restore cluster, not ordered prerequisites."],
  ["CAP-F122453871", "CAP-2BAFC73FFB", "Backup creation and restore safety are peers in one restore cluster, not ordered prerequisites."],
  ["CAP-CCA3A4522A", "CAP-F122453871", "Deployment recovery and archive creation are peers in one restore cluster."],
  ["CAP-2BAFC73FFB", "CAP-F122453871", "Restore safety and archive creation are peers in one restore cluster."],
  ["CAP-2BAFC73FFB", "CAP-CCA3A4522A", "Restore safety and deployment recovery are peers in one restore cluster."],
  ["CAP-25E7D01382", "CAP-480CDAE002", "Offline-state detection must not require an optional local-sync hub."],
  ["CAP-7506E394AB", "CAP-C7B707D720", "A location belongs to the selected store; creating another vendor-style account/store is not a prerequisite."],
  ["CAP-9708B96F30", "CAP-592BE3B314", "The native canonical cart must not depend on a browser-POS publication surface."],
  ["CAP-A51B302A7A", "CAP-592BE3B314", "Modifier configuration must not depend on a browser-POS publication surface."],
  ["CAP-D0F43729B0", "CAP-F122453871", "Mechanism-neutral on-chain recovery must not depend on BTCPay Docker archive creation."],
  ["CAP-D0F43729B0", "CAP-CCA3A4522A", "Mechanism-neutral on-chain recovery must not depend on BTCPay deployment-profile recovery."],
  ["CAP-D0F43729B0", "CAP-2BAFC73FFB", "Mechanism-neutral on-chain recovery must not depend on BTCPay Docker restore mechanics."],
  ["CAP-D43AD7DE36", "CAP-C46CC4D980", "Durable device/location assignment does not require a merchant-facing location-switch workflow."],
];
const removedEdgeSet = new Set(removedDependencyEdges.map(([source, target]) => `${source}:${target}`));

const laneFiles = expectedLaneFiles;
for (const laneFile of laneFiles) {
  if (!fs.existsSync(path.join(laneDirectory, laneFile))) {
    throw new Error(`missing required lane artifact ${laneFile}`);
  }
}
const normalizedById = new Map();
const contributingLaneFiles = [];
for (const laneFile of laneFiles) {
  const artifact = JSON.parse(fs.readFileSync(path.join(laneDirectory, laneFile), "utf8"));
  const artifactRows = rowsFromArtifact(artifact);
  if (artifactRows.length > 0) contributingLaneFiles.push(laneFile);
  for (const sourceRow of artifactRows) {
    const capabilityId = sourceRow.capability_id ?? sourceRow.id;
    if (!executionById.has(capabilityId)) throw new Error(`${laneFile}: unknown capability ID ${capabilityId}`);
    if (normalizedById.has(capabilityId)) throw new Error(`${laneFile}: duplicate capability ID ${capabilityId}`);
    const canonical = executionById.get(capabilityId);
    const audited = auditById.get(capabilityId);
    if (sourceRow.key !== canonical.key) throw new Error(`${laneFile}: key mismatch for ${capabilityId}`);

    const dependencyTokens = [
      ...(sourceRow.depends_on_capability_ids ?? []),
      ...(sourceRow.dependencies ?? []),
      ...(canonical.dependency_capability_ids ?? []),
    ];
    const dependencyIds = [];
    const gateKeys = [...(sourceRow.depends_on_gate_keys ?? []), ...(canonical.dependency_keys ?? [])];
    for (const token of dependencyTokens) {
      if (executionById.has(token)) dependencyIds.push(token);
      else if (executionByKey.has(token)) dependencyIds.push(executionByKey.get(token).id);
      else gateKeys.push(token);
    }
    for (const dependencyKey of canonical.dependency_keys ?? []) {
      if (executionByKey.has(dependencyKey)) dependencyIds.push(executionByKey.get(dependencyKey).id);
    }
    const normalizedDependencyIds = uniqueStrings(dependencyIds).filter(
      (id) => id !== capabilityId && !removedEdgeSet.has(`${capabilityId}:${id}`),
    );
    const normalizedGateKeys = uniqueStrings(gateKeys).filter(
      (key) => !executionById.has(key) && !executionByKey.has(key),
    );
    const externalDependencies = classifyExternalDependencies(normalizedGateKeys);
    const evidencePaths = uniqueStrings([
      ...(sourceRow.evidence_paths ?? []),
      ...(audited?.evidence_paths ?? []),
      ...(canonical.closure_evidence ?? []),
    ]).map(normalizeEvidencePath);
    for (const evidencePath of evidencePaths) {
      if (/^[a-z]+:\/\//i.test(evidencePath)) continue;
      const localPath = evidencePath.split("#", 1)[0];
      if (!fs.existsSync(path.join(repositoryRoot, localPath))) {
        throw new Error(`${laneFile}: missing evidence path ${evidencePath}`);
      }
    }

    normalizedById.set(capabilityId, {
      capability_id: capabilityId,
      key: canonical.key,
      category: canonical.category,
      capability: canonical.capability,
      merchant_outcome: canonical.merchant_outcome,
      current_status: audited?.canonical_status ?? canonical.status,
      benchmark_status: audited?.benchmark_status ?? canonical.benchmark_parity_status,
      layer: layerAliases.get(sourceRow.layer) ?? inferredSecurityLayer(sourceRow),
      foundation_candidate: normalizeFoundationCandidate(sourceRow),
      depends_on_capability_ids: normalizedDependencyIds,
      ...externalDependencies,
      shared_primitive: sourceRow.shared_primitive ?? sourceRow.foundation_class ?? sourceRow.implementation_cluster ?? "unclassified",
      implementation_cluster: sourceRow.implementation_cluster ?? sourceRow.foundation_class ?? "unclassified",
      rationale: sourceRow.rationale ?? sourceRow.dependency_rationale ?? sourceRow.recommendation_rationale ?? canonical.capability,
      parallelism_conflict_notes: sourceRow.parallelism_conflict_notes ?? sourceRow["parallelism/conflict notes"] ?? sourceRow.recommendation ?? "Coordinate changes to shared durable records and public contracts.",
      evidence_paths: evidencePaths,
      source_lane: laneFile,
    });
  }
}

const expectedIds = new Set(execution.capabilities.map((row) => row.id));
const actualIds = new Set(normalizedById.keys());
const missingIds = [...expectedIds].filter((id) => !actualIds.has(id)).sort();
const extraIds = [...actualIds].filter((id) => !expectedIds.has(id)).sort();
if (missingIds.length || extraIds.length) {
  throw new Error(`coverage mismatch: ${missingIds.length} missing, ${extraIds.length} extra\nmissing=${missingIds.join(",")}\nextra=${extraIds.join(",")}`);
}

const rows = [...normalizedById.values()].sort((a, b) => a.capability_id.localeCompare(b.capability_id));
const incomingCounts = Object.fromEntries(rows.map((row) => [row.capability_id, 0]));
for (const row of rows) {
  if (row.depends_on_capability_ids.includes(row.capability_id)) throw new Error(`self dependency: ${row.capability_id}`);
  for (const dependencyId of row.depends_on_capability_ids) {
    if (!executionById.has(dependencyId)) throw new Error(`${row.capability_id}: unknown dependency ${dependencyId}`);
    incomingCounts[dependencyId] += 1;
  }
}

function findCycleComponents(graphRows) {
  const byId = new Map(graphRows.map((row) => [row.capability_id, row]));
  const indexById = new Map();
  const lowLinkById = new Map();
  const stack = [];
  const onStack = new Set();
  const components = [];
  let nextIndex = 0;

  function visit(capabilityId) {
    indexById.set(capabilityId, nextIndex);
    lowLinkById.set(capabilityId, nextIndex);
    nextIndex += 1;
    stack.push(capabilityId);
    onStack.add(capabilityId);
    for (const dependencyId of byId.get(capabilityId).depends_on_capability_ids) {
      if (!indexById.has(dependencyId)) {
        visit(dependencyId);
        lowLinkById.set(
          capabilityId,
          Math.min(lowLinkById.get(capabilityId), lowLinkById.get(dependencyId)),
        );
      } else if (onStack.has(dependencyId)) {
        lowLinkById.set(
          capabilityId,
          Math.min(lowLinkById.get(capabilityId), indexById.get(dependencyId)),
        );
      }
    }
    if (lowLinkById.get(capabilityId) !== indexById.get(capabilityId)) return;
    const component = [];
    while (stack.length > 0) {
      const member = stack.pop();
      onStack.delete(member);
      component.push(member);
      if (member === capabilityId) break;
    }
    if (component.length > 1) components.push(component.sort());
  }

  for (const row of graphRows) {
    if (!indexById.has(row.capability_id)) visit(row.capability_id);
  }
  return components;
}

const cycleComponents = findCycleComponents(rows);
if (cycleComponents.length > 0) {
  throw new Error(`dependency cycles: ${JSON.stringify(cycleComponents)}`);
}

const result = {
  title: "OpenBMT 502-capability dependency and foundation graph",
  as_of: "2026-11-17",
  method: [
    "Every canonical capability was classified independently inside a non-overlapping domain lane.",
    "Canonical status comes from the full-audit row when present and otherwise from the canonical execution ledger; lane labels never override it.",
    "Layers describe implementation dependency, not product value or proof of support.",
    "Capability edges are reviewed architectural sequencing judgments, not code-discovered coupling; raw centrality must not select foundations by itself.",
    "Foundation candidates are proposals; tranche selection requires architecture and safety review.",
  ],
  layer_definitions: {
    L0_invariant: "Safety or correctness invariant that dependent workflows must preserve.",
    L1_shared_foundation: "Shared identity, authority, money, state, or storage primitive.",
    L2_core_workflow: "Merchant workflow assembled from L0/L1 foundations.",
    L3_enhancement: "Valuable extension that is not required to make the core safe.",
    L4_vertical_or_adapter: "Vertical-specific behavior, provider adapter, or benchmark-specific leaf.",
  },
  summary: {
    total_capabilities: rows.length,
    source_lanes: contributingLaneFiles.length,
    by_layer: countBy(rows.map((row) => row.layer)),
    by_status: countBy(rows.map((row) => row.current_status)),
    by_category: countBy(rows.map((row) => row.category)),
    proposed_foundation_candidates: rows.filter((row) => row.foundation_candidate).length,
    dependency_edges: rows.reduce((total, row) => total + row.depends_on_capability_ids.length, 0),
  },
  dependency_centrality: Object.entries(incomingCounts)
    .map(([capability_id, dependent_count]) => ({ capability_id, dependent_count }))
    .filter((row) => row.dependent_count > 0)
    .sort((a, b) => b.dependent_count - a.dependent_count || a.capability_id.localeCompare(b.capability_id)),
  capabilities: rows,
  validation: {
    expected_capabilities: expectedIds.size,
    included_capabilities: rows.length,
    missing_capability_ids: missingIds,
    extra_capability_ids: extraIds,
    duplicate_capability_ids: 0,
    dependency_cycle_components: cycleComponents,
    evidence_paths_checked_from_repository_root: true,
    required_lane_files: expectedLaneFiles,
    removed_semantically_invalid_edges: removedDependencyEdges.map(([source, target, reason]) => ({
      source,
      target,
      reason,
    })),
    canonical_status_sources: [
      path.relative(repositoryRoot, executionPath),
      path.relative(repositoryRoot, auditPath),
    ],
  },
};

if (selectionPath) {
  const selection = JSON.parse(fs.readFileSync(selectionPath, "utf8"));
  const selectedIds = selection.groups.flatMap((group) => group.capability_ids);
  const uniqueSelectedIds = new Set(selectedIds);
  if (selectedIds.length !== uniqueSelectedIds.size) {
    throw new Error("foundation tranche contains duplicate capability IDs");
  }
  for (const capabilityId of selectedIds) {
    if (!normalizedById.has(capabilityId)) {
      throw new Error(`foundation tranche contains unknown capability ID ${capabilityId}`);
    }
  }
  const outsidePrerequisiteEdges = selectedIds.flatMap((capabilityId) => {
    const source = normalizedById.get(capabilityId);
    return source.depends_on_capability_ids
      .filter((dependencyId) => !uniqueSelectedIds.has(dependencyId))
      .map((dependencyId) => {
        const dependency = normalizedById.get(dependencyId);
        return {
          source_capability_id: capabilityId,
          source_key: source.key,
          prerequisite_capability_id: dependencyId,
          prerequisite_key: dependency.key,
          prerequisite_status: dependency.current_status,
        };
      });
  });
  result.recommended_first_tranche = {
    selection_source: path.relative(repositoryRoot, selectionPath),
    immediate_code_slice: selection.immediate_code_slice,
    total_capabilities: selectedIds.length,
    shared_primitives: selection.groups.length,
    by_status: countBy(selectedIds.map((id) => normalizedById.get(id).current_status)),
    outside_prerequisite_anchors: outsidePrerequisiteEdges,
    groups: selection.groups.map((group) => ({
      ...group,
      capabilities: group.capability_ids.map((id) => {
        const row = normalizedById.get(id);
        return {
          capability_id: id,
          key: row.key,
          current_status: row.current_status,
          layer: row.layer,
        };
      }),
    })),
  };
  result.validation.recommended_tranche_ids_resolve = true;
  result.validation.recommended_tranche_unique_ids = uniqueSelectedIds.size;
}

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
fs.writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`);
console.log(JSON.stringify(result.summary, null, 2));
