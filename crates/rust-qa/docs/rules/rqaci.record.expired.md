# rqaci.record.expired

## What it flags

A project-record exception whose `review_by` date (UTC) is in the past.

## Why

An exception is decided under conditions that change: the dependency gets fixed, the code moves,
the rule improves. Without a deadline nobody looks at an entry again. A review date makes every
exception temporary by default and forces a fresh decision.

## How to fix correctly

First try to remove it: fix the code and delete the entry. If the exception still holds, re-decide
it: re-read the justification, set `decided_on` to today, and set a new `review_by` within the
horizon for the entry's trust tier (`rust-qa record explain <rule>` prints it). Pushing the date
forward without re-reading the justification is the failure this rule exists to prevent.

## Why syntax suffices

It compares a date in the record with today's date.

## Suppression

Not suppressible.
