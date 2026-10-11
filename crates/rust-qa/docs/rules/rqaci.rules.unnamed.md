# rqaci.rules.unnamed

## What it flags

A rule file in the `ruleDirs` of `sgconfig.yml` holding a rule with no `id`.

## Why

ast-grep runs such a rule under the file's name, so its findings print an identifier that no
listing names, no page documents and no test case proves: `rqaci.docs.dangling` and
`rqaci.rules.untested` never ask for either, and `rust-qa rule-doc` resolves nothing for it.

## How to fix correctly

Give the rule a stable, namespaced `id` (outside the `rqaci.` namespace), then add its page under the
documentation directory and its test case in a `testConfigs` directory.

## Why syntax suffices

It reads the rule files' YAML.

## Suppression

Not suppressible; name the rule.
