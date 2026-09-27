import { expect, it } from 'vitest'
import { collectConditions, hasCurrentCondition } from '../../e2e/support/resource-observation'

const accepted = { type: 'Accepted', status: 'True' }

it('collects each controller native payload without trusting a flat envelope fallback', () => {
  const parent = { ...accepted, observedGeneration: 1 }
  const ancestor = { ...accepted, observedGeneration: 2 }
  const listener = { type: 'Programmed', status: 'True', observedGeneration: 2 }
  expect(collectConditions({ conditions: [accepted], controllers: [
    { controllerName: 'one', status: { parents: [{ conditions: [parent] }] } },
    { controllerName: 'two', status: { ancestors: [{ conditions: [ancestor] }], listeners: [{ conditions: [listener] }] } },
    { status: { conditions: [accepted] } }, null,
  ] })).toEqual([parent, listener, ancestor])
  expect(collectConditions({ controllers: null, conditions: [accepted] })).toEqual([])
})

it.each([undefined, 1, 3])('does not accept observation %s for current generation 2', (observedGeneration) => {
  expect(hasCurrentCondition({ generation: 2, spec: {}, conditions: [{ ...accepted, observedGeneration }] }, accepted)).toBe(false)
})

it('accepts matching current observations and versionless standalone conditions', () => {
  expect(hasCurrentCondition({ generation: 2, spec: {}, conditions: [{ ...accepted, observedGeneration: 2 }] }, accepted)).toBe(true)
  expect(hasCurrentCondition({ spec: {}, conditions: [accepted] }, accepted)).toBe(true)
  expect(hasCurrentCondition({ spec: {}, conditions: [accepted] }, { ...accepted, reason: 'Conflicted' })).toBe(false)
})
