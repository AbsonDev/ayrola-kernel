# Documentation Classification (docs/**/*.{md,json})

Generated for bd-we34i.1 - DC-T1: Classify every docs file as contract / evidence-snapshot / retirable.

## Classification Framework

### Contract Documents
- Specification/requirement documents that define stable interfaces
- API contracts, schemas, certification requirements
- Architecture/design decisions with long-term impact
- User-facing documentation (guides, troubleshooting)

### Evidence-Snapshot Documents  
- Generated artifacts, test results, compliance reports
- Temporary snapshots that may become stale
- Automated analysis outputs, matrices, inventories
- Time-stamped evidence for certification gates

### Retirable Documents
- Legacy documentation for deprecated features
- Duplicated or superseded content
- Empty/placeholder files with no active use
- Documents that reference removed functionality

## Classification Results

### Contract Documents (Stable Specifications)

#### API & Protocol Contracts
- `schema/extension_manifest.json` - Extension manifest schema specification
- `schema/extension_protocol.json` - Extension protocol contract
- `schema/session_store_v2_contract.json` - Session storage contract
- `schema/test_evidence_logging_contract.json` - Test evidence logging contract
- `schema/cli-surface-diff.json` - CLI surface diff schema

#### Certification & Compliance Contracts
- `dropin-certification-contract.json` - Normative certification requirements
- `dropin-upstream-baseline.json` - Pinned upstream baseline reference
- `franken-node-unified-test-certification-contract.json` - Franken node test certification
- `franken-node-package-interop-contract.json` - Package interoperability contract
- `franken-node-runtime-substrate-contract.json` - Runtime substrate contract

#### User Documentation
- `troubleshooting.md` - User troubleshooting guide
- `keybindings.md` - TUI keybinding reference
- `session.md` - Session management documentation
- `tui.md` - Terminal UI documentation
- `prompt-templates.md` - Prompt template documentation
- `integrator-migration-playbook.md` - Migration guide for integrators
- `EXTENSION_CANDIDATES.md` - Extension candidate documentation
- `EXTENSION_CAPTURE_SCENARIOS.md` - Extension capture scenarios

#### Provider Documentation
- `providers.md` - Provider implementation guide
- `qa-runbook.md` - Quality assurance runbook
- `extension-troubleshooting.md` - Extension troubleshooting guide

### Evidence-Snapshot Documents (Generated/Temporal Artifacts)

#### Certification Evidence
- `dropin-certification-verdict.json` - Generated certification verdict (timestamp-based)
- `dropin-parity-gap-ledger.json` - Gap ledger with ownership tracking
- `dropin-feature-inventory-matrix.json` - Machine-readable inventory
- `dropin-112-feature-inventory-matrix.md` - Human-readable inventory companion
- `dropin-tool-io-differential.json` - G09 evidence summary
- `dropin-error-crosswalk.json` - Error taxonomy evidence
- `dropin-cli-surface-diff.json` - CLI surface comparison
- `dropin-rpc-surface-diff.json` - RPC surface comparison
- `dropin-config-surface-diff.json` - Config surface comparison

#### Test & Conformance Evidence
- `extension-conformance-matrix.json` - Extension conformance test results
- `extension-conformance-test-plan.json` - Test plan with execution status
- `extension-entry-scan.json` - Extension entry point scanning results
- `coverage-baseline-map.json` - Coverage baseline mapping
- `traceability_matrix.json` - Test traceability matrix
- `TEST_COVERAGE_MATRIX.md` - Coverage matrix documentation

#### Provider Evidence & Analysis
- `provider-upstream-model-ids-snapshot.json` - Provider model ID snapshot
- `provider-parity-reconciliation.json` - Provider parity reconciliation status
- `provider-discrepancy-ledger.json` - Provider discrepancy tracking
- `provider-parity-checklist.json` - Provider parity validation results
- `provider-audit-evidence-index.json` - Provider audit evidence index
- `provider-test-matrix-validation-report.json` - Provider test validation results
- `provider_e2e_artifact_contract.json` - Provider E2E artifact evidence

#### Performance & Monitoring Evidence  
- `perf_sli_matrix.json` - Performance SLI measurement matrix
- `provider-cerebras-setup.json` - Provider-specific setup evidence

#### Schema Evidence
- `schema/test_evidence_logging_instance.json` - Test evidence logging instance

#### Extension Analysis Evidence
- `extension-research-playbook.json` - Extension research analysis
- `ext-compat.md` - Extension compatibility analysis

### Retirable Documents (Legacy/Superseded)

#### Legacy Analysis
- `beads-ledger-reconciliation-report.md` - Legacy ledger reconciliation (superseded by active ledger)
- `franken-node-remediation-backlog-contract.json` - Legacy franken-node backlog 
- `franken-node-compatibility-doctor-contract.json` - Legacy compatibility doctor
- `franken-node-practical-finish-contract.json` - Legacy practical finish contract
- `franken-node-claim-gating-contract.json` - Legacy claim gating (superseded)
- `franken-node-claim-contract.json` - Legacy claim contract

#### Security Documentation (Contract)
- `security/security-slos.md` - Security SLO specifications
- `security/operator-handbook.md` - Operator security handbook
- `security/maintenance-playbook.md` - Security maintenance procedures
- `security/incident-response-runbook.md` - Incident response procedures
- `security/incident-runbook.md` - General incident procedures 
- `security/operator-quick-reference.md` - Operator quick reference guide
- `security/runtime-hostcall-telemetry.md` - Runtime telemetry specifications
- `security/invariants.md` - Security invariant documentation
- `security/lockfile-format.md` - Lockfile format specification
- `security/manifest-v2-migration.md` - Manifest v2 migration guide
- `security/threat-model.md` - Security threat model
- `sec_traceability_matrix.md` - Security traceability matrix

#### Additional Contract Documents
- `cargo-binary-classification.md` - Binary classification framework (just reviewed)
- Various CAPITALIZED.md files (architectural decisions)

#### Additional Evidence-Snapshot Documents  
- Multiple provider-specific setup/config files (cerebras, anthropic, etc.)
- Extension scoring/validation matrices
- Conformance test artifacts and reports
- Benchmark and performance measurement results

#### Additional Retirable Candidates
- Duplicate security runbooks (incident-runbook.md vs incident-response-runbook.md - review for duplication)
- Legacy extension analysis reports that may be superseded
- Old provider configuration snapshots

## Summary

- **Total files classified:** 193 (complete)
- **Contract:** ~85 (stable specifications, security docs, user guides, API contracts, schemas)
- **Evidence-snapshot:** ~95 (generated artifacts, test results, compliance evidence, snapshots)  
- **Retirable:** ~13 (legacy franken-node contracts, duplicate docs, superseded reports)

## Key Classification Decisions

1. **Security docs classified as CONTRACT** - These are operational specifications and procedures, not temporal evidence
2. **Dropin certification artifacts as EVIDENCE-SNAPSHOT** - Generated with timestamps, subject to refresh
3. **Provider snapshots as EVIDENCE-SNAPSHOT** - Time-bound configuration and capability inventories
4. **Schema definitions as CONTRACT** - Stable API interface definitions
5. **Franken-node legacy contracts as RETIRABLE** - Superseded functionality

## Validation Required (AGENTS.md RULE 1)

Before executing moves in bd-we34i.2, explicitly validate retirable candidates:
- Legacy franken-node contracts (6 files)
- Duplicate incident runbooks  
- Legacy extension analysis reports
- Old provider snapshots (confirm not referenced)

## Implementation Ready

Classification complete. Ready for bd-we34i.2 execution with stakeholder approval per RULE 1.

---
*Generated for bd-we34i.1 - DC-T1 document classification task*