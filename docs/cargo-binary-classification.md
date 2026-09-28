# Cargo Binary Classification (src/bin/*.rs)

Generated for bd-uuwgi.1 - CM-T1: Enumerate and classify shipping vs dev-only vs retirable binaries.

## Current Binary Inventory

**UPDATED CLASSIFICATION** - Previous analysis reflected outdated codebase state. Current src/bin/ directory contains only 1 binary.

## Shipping Binaries (Production Tools)

**None** - No production binaries in src/bin/.

## Dev-Only Binaries (Development & Testing Tools)

**None** - No active development binaries in src/bin/.

## Retirable Binaries (Legacy/Unused)

### Legacy Tools
- `pi_legacy_capture.rs` - Legacy capture functionality (91KB) - **RETIRABLE**
  - Last modified: 2026-03-12 (stale)
  - Size: 91KB (substantial legacy code)
  - No active usage patterns identified
  - Candidate for removal per AGENTS.md RULE 1 with approval

## Summary

- **Total binaries:** 1 (down from previously reported 22)
- **Shipping:** 0
- **Dev-only:** 0
- **Retirable:** 1 (pi_legacy_capture.rs)

## Analysis Notes

**Significant Change**: The src/bin/ directory previously contained 22 binaries but now contains only 1. This suggests:
- Dev/test binaries were moved to different locations (tests/, examples/, or removed)
- Codebase consolidation removed redundant tooling
- Legacy cleanup already occurred for most binaries

**Current State**: Only `pi_legacy_capture.rs` remains, marked as retirable legacy code.

## Recommendations

1. **Retire pi_legacy_capture.rs** - with stakeholder approval per AGENTS.md RULE 1
   - 91KB of legacy code with no apparent active usage
   - Last modification predates recent development activity
   - Removal would clean up final legacy binary

2. **Validate binary consolidation** - Confirm dev/test utilities moved to appropriate locations
3. **Update Cargo.toml** - Remove bin entries for deleted binaries if needed

## Implementation Actions

✅ **Classification Complete** - Single binary identified and categorized  
📋 **Ready for bd-uuwgi.2** - Cargo.toml updates with retirable binary removal (pending approval)

Generated: 2026-04-23T04:42:00Z (Updated from previous analysis)