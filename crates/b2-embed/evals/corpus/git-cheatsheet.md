---
type: note
title: Git cheatsheet
b2id: 01JEVAL2GITCH000000000000A
---
Commands worth pinning down, collected to stop the same lookups recurring.

## Undoing history

Take back the last commit but keep its edits in the index:

```sh
git reset --soft HEAD~1
```

Drop the edits too (destructive — the working tree is overwritten):

```sh
git reset --hard HEAD~1
```

Rewrite the last commit message without touching its content:

```sh
git commit --amend
```

## Recovering lost work

Every position HEAD has occupied is still in the journal:

```sh
git reflog
git checkout -b rescued <sha-from-reflog>
```

A branch deleted with `-D` stays reachable this way for as long as the reflog
keeps its entry — around ninety days by default.

## Housekeeping

```sh
git stash push -m "wip"     # shelve the working tree
git stash pop               # take it back
git clean -nd               # preview untracked deletions, then -fd
```

The dry-run flag first, always: `clean` and `reset --hard` are the two commands
here that discard work irreversibly.
