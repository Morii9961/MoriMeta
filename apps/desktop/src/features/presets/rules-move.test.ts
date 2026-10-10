// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { moveRule } from './model'

describe('moving a rule by dragging it', () => {
  const r = ['a', 'b', 'c', 'd']
  it('drops before the rule under the line, counting from the list without it', () => {
    expect(moveRule(r, 3, 0)).toEqual(['d', 'a', 'b', 'c'])
    expect(moveRule(r, 0, 4)).toEqual(['b', 'c', 'd', 'a'])
    expect(moveRule(r, 0, 2)).toEqual(['b', 'a', 'c', 'd'])
    expect(moveRule(r, 2, 1)).toEqual(['a', 'c', 'b', 'd'])
  })
  it('changes nothing when dropped next to itself or outside the list', () => {
    expect(moveRule(r, 1, 1)).toEqual(r)
    expect(moveRule(r, 1, 2)).toEqual(r)
    expect(moveRule(r, 5, 0)).toBe(r)
    expect(moveRule(r, 0, 9)).toBe(r)
  })
  it('keeps every rule exactly once', () => {
    for (let from = 0; from < r.length; from++) {
      for (let to = 0; to <= r.length; to++) expect([...moveRule(r, from, to)].sort()).toEqual(r)
    }
  })
})