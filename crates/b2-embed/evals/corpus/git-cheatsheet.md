---
type: note
title: Git cheatsheet
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

Every position `HEAD` has occupied on this machine is still in its journal:

```sh
git reflog
git checkout -b rescued <sha-from-reflog>
```

A branch deleted with `-D` can be found this way only if it was checked out at
some point — the walk above reads `HEAD`'s journal, and a branch that never
held `HEAD` needs its tip dug out by other means. Entries expire too: about
ninety days for reachable history and thirty for unreachable, by default.

## Housekeeping

```sh
git stash push -m "wip"     # shelve the working tree
git stash pop               # take it back
git clean -nd               # preview untracked deletions, then -fd
```

The dry-run flag first, always: `clean` and `reset --hard` are the two commands
here that discard work irreversibly.
