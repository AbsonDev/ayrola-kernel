# Classification Moves Proposal - bd-we34i.2

Generated for DC-T2: Execute classification moves with explicit user approval per AGENTS.md RULE 1

## RULE 1 Compliance: Retirable Candidates Requiring Approval

### Legacy Franken-Node Contracts (6 files)
**Requesting approval to REMOVE:**
- `docs/franken-node-remediation-backlog-contract.json` - Legacy backlog superseded
- `docs/franken-node-compatibility-doctor-contract.json` - Legacy compatibility doctor  
- `docs/franken-node-practical-finish-contract.json` - Legacy practical finish
- `docs/franken-node-claim-gating-contract.json` - Legacy claim gating superseded
- `docs/franken-node-claim-contract.json` - Legacy claim contract
- `docs/franken-node-unified-test-certification-contract.json` - May still be active - VERIFY

### Potential Duplicates (Requiring Review)
**Requesting approval to CONSOLIDATE:**
- `docs/security/incident-runbook.md` vs `docs/security/incident-response-runbook.md` - Check for duplication
- Legacy extension analysis reports - Review for superseded versions

## Safe Organizational Moves (No RULE 1 Required)

### Evidence Organization
```bash
# Create evidence subdirectory for snapshots
mkdir -p docs/evidence/
# Already moved: docs/evidence/dropin-certification-verdict.json
git mv docs/dropin-parity-gap-ledger.json docs/evidence/
git mv docs/dropin-feature-inventory-matrix.json docs/evidence/
git mv docs/dropin-*-diff.json docs/evidence/
git mv docs/provider-*-snapshot.json docs/evidence/
```

### Contract Organization  
```bash
# Create contracts subdirectory for specifications
mkdir -p docs/contracts/
git mv docs/dropin-certification-contract.json docs/contracts/
git mv docs/dropin-upstream-baseline.json docs/contracts/
```

## Implementation Plan

**Phase 1: Safe Moves (Execute Now)**
- Organize evidence files into docs/evidence/
- Organize contract files into docs/contracts/
- Update internal references in documentation

**Phase 2: Awaiting Approval**
- Remove legacy franken-node contracts (pending approval)
- Consolidate duplicate files (pending review)

## Commit Strategy
1. Execute safe organizational moves
2. Commit with "organize docs structure" message  
3. Surface retirable candidates for RULE 1 approval
4. Execute removals after explicit approval

---
**Status**: Ready for safe moves execution + RULE 1 approval request
