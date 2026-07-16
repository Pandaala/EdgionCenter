import { describe, expect, it } from 'vitest'
import * as yaml from 'js-yaml'
import { dumpYaml } from './yaml-utils'

describe('lossless YAML serialization', () => {
  it('preserves explicit empty, false, zero, and null values', () => {
    const value = {
      emptyString: '',
      emptyArray: [],
      emptyObject: {},
      disabled: false,
      count: 0,
      nullable: null,
    }

    expect(yaml.load(dumpYaml(value))).toEqual(value)
  })
})
