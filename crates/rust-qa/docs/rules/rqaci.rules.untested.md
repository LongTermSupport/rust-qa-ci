# rqaci.rules.untested

## What it flags

A project ast-grep rule at `severity: error` (one found in the `ruleDirs` of `sgconfig.yml`) with no
test case that holds both `valid` code, on which the rule must stay silent, and `invalid` code, on
which it must fire, in a `testDir` that `testConfigs` in `sgconfig.yml` names.

## Why

A rule is a detector only once it has been seen to fire on the defect and to stay silent on correct
code. Without both halves, a pattern that never matches passes the gate as surely as a rule that
works, and the defence is believed in without evidence. When `sgconfig.yml` declares `testConfigs`,
the `ast-grep` detector also runs `ast-grep test --skip-snapshot-tests`, which fails a case whose
code does not behave as the case says.

## How to fix correctly

Add a test file to a `testConfigs` directory:

```yaml
id: <rule id>
valid:
  - <code the rule must not report>
invalid:
  - <code the rule must report>
```

Take the `invalid` code from the defect the rule was written for, and the `valid` code from its
correct construction.

## Why syntax suffices

It reads the rule and test files' YAML; the cases themselves are run by `ast-grep test`.

## Suppression

Not suppressible; write the test case.
