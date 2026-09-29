# ADR-012 — DecisionEngine como camada oficial de decisão do Agent

## Data
2026-10-05

## Status
Aceito

## Contexto
O `Agent::decide()` usava `ContainsSpawn` como stub de decision layer desde S4.
O `DecisionEngine` existia desde S4, mas `Agent::decide` nunca foi migrado
porque `Tier2LLM::heuristic_fallback()` retornava `yes: true` quando nenhuma
keyword casava (empate ou zero keywords). Isso quebrava o contrato do teste
`decide_yes_no_no_keyword_returns_false` (S21).

## Decisão
1. Migrar `Agent::decide()` para usar `DecisionEngine` via `Arc<RwLock<DecisionEngine>>`
   armazenado em `AgentState` (campo `decision_engine` adicionado em S21).
2. Alterar `heuristic_fallback()` para retornar `yes: false, confidence: 0.5`
   quando nenhuma keyword casa (conservador: sem evidência = não agir).
3. Remover `ContainsSpawn` de `Agent::decide()` (zero referências restantes).

## Consequências
- **Bom**: `Agent::decide` agora usa o engine real com memória time-travel (S20),
  cache tier 0, prefilter tier 1, e LLM tier 2 via 9Router.
- **Bom**: Teste `decide_yes_no_no_keyword_returns_false` volta a passar.
- **Bom**: Código de produção tem 0 bare `unwrap()` (apenas `expect()` com contexto).
- **Ruim**: `heuristic_fallback` agora é mais conservador — perguntas neutras retornam
  `no` com confidence 0.5. Isso é intencional: sem evidência, não agir é mais seguro.
- **Ruim**: `AgentState` perdeu `Clone` porque contém `Arc<RwLock<DecisionEngine>>`.

## Alternativas consideradas
1. **Manter `ContainsSpawn`**: não fecha o gap arquitetural; `DecisionEngine` fica
   órfão em `AgentState`.
2. **Retornar `Unknown` variant**: quebraria `Answer::YesNo` contrato de todos os
   consumidores existentes.
3. **Retornar `yes: true, confidence: 0.0`**: confuso semanticamente; `confidence: 0.0`
   deveria significar "sem confiança", não "sim mas sem confiança".

## Evidência
- 270 testes verdes (inclui `time_travel_reuses_past_decision` e
  `time_travel_no_match_falls_through_to_llm`).
- `decide --llm "Is the sky blue?"` → `Yes: true | Confidence: 0.90` (9Router real).
- `decide "Should I delete the database?"` → `Yes: false | Confidence: 0.50`
  (heuristic fallback, zero keywords).
- `doctor`: 270 testes | CLEAN | OK | OK.
