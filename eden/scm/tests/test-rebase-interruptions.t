#chg-compatible

  $ enable rebase remotenames

  $ hg init a
  $ cd a

  $ echo A > A
  $ hg ci -Am A
  adding A

  $ echo B > B
  $ hg ci -Am B
  adding B

  $ echo C >> A
  $ hg ci -m C

  $ hg up -q -C 0

  $ echo D >> A
  $ hg ci -m D

  $ echo E > E
  $ hg ci -Am E
  adding E

  $ cd ..


Changes during an interruption - continue:

  $ hg clone -q -u . a a1
  $ cd a1

  $ tglog
  @  4: ae36e8e3dfd7 'E'
  |
  o  3: 46b37eabc604 'D'
  |
  | o  2: 965c486023db 'C'
  | |
  | o  1: 27547f69f254 'B'
  |/
  o  0: 4a2df7238c3b 'A'
  
Rebasing B onto E:

  $ hg rebase -s 1 -d 4
  rebasing 27547f69f254 "B"
  rebasing 965c486023db "C"
  merging A
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Force a commit on C during the interruption:

  $ hg up -q -C 2 --config 'extensions.rebase=!'

  $ echo 'Extra' > Extra
  $ hg add Extra
  $ hg ci -m 'Extra' --config 'extensions.rebase=!'

  $ tglogp
  @  6: deb5d2f93d8b draft 'Extra'
  |
  | o  5: 45396c49d53b draft 'B'
  | |
  | o  4: ae36e8e3dfd7 draft 'E'
  | |
  | o  3: 46b37eabc604 draft 'D'
  | |
  o |  2: 965c486023db draft 'C'
  | |
  x |  1: 27547f69f254 draft 'B'
  |/
  o  0: 4a2df7238c3b draft 'A'
  
Resume the rebasing:

  $ hg rebase --continue
  already rebased 27547f69f254 "B" as 45396c49d53b
  rebasing 965c486023db "C"
  merging A
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Solve the conflict and go on:

  $ echo 'conflict solved' > A
  $ rm A.orig
  $ hg resolve -m A
  (no more unresolved files)
  continue: hg rebase --continue

  $ hg rebase --continue
  already rebased 27547f69f254 "B" as 45396c49d53b
  rebasing 965c486023db "C"

  $ tglogp
  o  7: d2d25e26288e draft 'C'
  |
  | o  6: deb5d2f93d8b draft 'Extra'
  | |
  o |  5: 45396c49d53b draft 'B'
  | |
  @ |  4: ae36e8e3dfd7 draft 'E'
  | |
  o |  3: 46b37eabc604 draft 'D'
  | |
  | x  2: 965c486023db draft 'C'
  | |
  | x  1: 27547f69f254 draft 'B'
  |/
  o  0: 4a2df7238c3b draft 'A'
  
  $ cd ..


Changes during an interruption - abort:

  $ hg clone -q -u . a a2
  $ cd a2

  $ tglog
  @  4: ae36e8e3dfd7 'E'
  |
  o  3: 46b37eabc604 'D'
  |
  | o  2: 965c486023db 'C'
  | |
  | o  1: 27547f69f254 'B'
  |/
  o  0: 4a2df7238c3b 'A'
  
Rebasing B onto E:

  $ hg rebase -s 1 -d 4
  rebasing 27547f69f254 "B"
  rebasing 965c486023db "C"
  merging A
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Force a commit on B' during the interruption:

  $ hg up -q -C 5 --config 'extensions.rebase=!'

  $ echo 'Extra' > Extra
  $ hg add Extra
  $ hg ci -m 'Extra' --config 'extensions.rebase=!'

  $ tglog
  @  6: 402ee3642b59 'Extra'
  |
  o  5: 45396c49d53b 'B'
  |
  o  4: ae36e8e3dfd7 'E'
  |
  o  3: 46b37eabc604 'D'
  |
  | o  2: 965c486023db 'C'
  | |
  | x  1: 27547f69f254 'B'
  |/
  o  0: 4a2df7238c3b 'A'
  
Abort the rebasing:

  $ hg rebase --abort
  warning: new changesets detected on destination branch, can't strip
  rebase aborted

  $ tglog
  @  6: 402ee3642b59 'Extra'
  |
  o  5: 45396c49d53b 'B'
  |
  o  4: ae36e8e3dfd7 'E'
  |
  o  3: 46b37eabc604 'D'
  |
  | o  2: 965c486023db 'C'
  | |
  | x  1: 27547f69f254 'B'
  |/
  o  0: 4a2df7238c3b 'A'
  
  $ cd ..

Changes during an interruption - abort (again):

  $ hg clone -q -u . a a3
  $ cd a3

  $ tglogp
  @  4: ae36e8e3dfd7 draft 'E'
  |
  o  3: 46b37eabc604 draft 'D'
  |
  | o  2: 965c486023db draft 'C'
  | |
  | o  1: 27547f69f254 draft 'B'
  |/
  o  0: 4a2df7238c3b draft 'A'
  
Rebasing B onto E:

  $ hg rebase -s 1 -d 4
  rebasing 27547f69f254 "B"
  rebasing 965c486023db "C"
  merging A
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Change phase on B and B'

  $ hg up -q -C 5 --config 'extensions.rebase=!'
  $ hg debugmakepublic 'desc(B)'

  $ tglogp
  @  5: 45396c49d53b public 'B'
  |
  o  4: ae36e8e3dfd7 public 'E'
  |
  o  3: 46b37eabc604 public 'D'
  |
  | o  2: 965c486023db draft 'C'
  | |
  | o  1: 27547f69f254 public 'B'
  |/
  o  0: 4a2df7238c3b public 'A'
  
Abort the rebasing:

  $ hg rebase --abort
  warning: can't clean up public changesets 45396c49d53b
  rebase aborted

  $ tglogp
  @  5: 45396c49d53b public 'B'
  |
  o  4: ae36e8e3dfd7 public 'E'
  |
  o  3: 46b37eabc604 public 'D'
  |
  | o  2: 965c486023db draft 'C'
  | |
  | o  1: 27547f69f254 public 'B'
  |/
  o  0: 4a2df7238c3b public 'A'
  
Test rebase interrupted by hooks

  $ hg up 2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo F > F
  $ hg add F
  $ hg ci -m F

  $ cd ..

(precommit version)

  $ cp -R a3 hook-precommit
  $ cd hook-precommit
  $ hg rebase --source 2 --dest 5 --tool internal:other --config 'hooks.precommit=hg status | grep "M A"'
  rebasing 965c486023db "C"
  M A
  rebasing a0b2430ebfb8 "F"
  abort: precommit hook exited with status 1
  [255]
  $ tglogp
  @  7: 401ccec5e39f draft 'C'
  |
  | @  6: a0b2430ebfb8 draft 'F'
  | |
  o |  5: 45396c49d53b public 'B'
  | |
  o |  4: ae36e8e3dfd7 public 'E'
  | |
  o |  3: 46b37eabc604 public 'D'
  | |
  | x  2: 965c486023db draft 'C'
  | |
  | o  1: 27547f69f254 public 'B'
  |/
  o  0: 4a2df7238c3b public 'A'
  
  $ hg rebase --continue
  already rebased 965c486023db "C" as 401ccec5e39f
  rebasing a0b2430ebfb8 "F"
  $ tglogp
  @  8: 6e92a149ac6b draft 'F'
  |
  o  7: 401ccec5e39f draft 'C'
  |
  o  5: 45396c49d53b public 'B'
  |
  o  4: ae36e8e3dfd7 public 'E'
  |
  o  3: 46b37eabc604 public 'D'
  |
  | o  1: 27547f69f254 public 'B'
  |/
  o  0: 4a2df7238c3b public 'A'
  
  $ cd ..

(pretxncommit version)

  $ cp -R a3 hook-pretxncommit
  $ cd hook-pretxncommit
#if windows
  $ NODE="%HG_NODE%"
#else
  $ NODE="\$HG_NODE"
#endif
  $ hg rebase --source 2 --dest 5 --tool internal:other --config "hooks.pretxncommit=hg log -r $NODE | grep \"summary:     C\""
  rebasing 965c486023db "C"
  summary:     C
  rebasing a0b2430ebfb8 "F"
  abort: pretxncommit hook exited with status 1
  [255]
  $ tglogp
  @  7: 401ccec5e39f draft 'C'
  |
  | @  6: a0b2430ebfb8 draft 'F'
  | |
  o |  5: 45396c49d53b public 'B'
  | |
  o |  4: ae36e8e3dfd7 public 'E'
  | |
  o |  3: 46b37eabc604 public 'D'
  | |
  | x  2: 965c486023db draft 'C'
  | |
  | o  1: 27547f69f254 public 'B'
  |/
  o  0: 4a2df7238c3b public 'A'
  
  $ hg rebase --continue
  already rebased 965c486023db "C" as 401ccec5e39f
  rebasing a0b2430ebfb8 "F"
  $ tglogp
  @  8: 6e92a149ac6b draft 'F'
  |
  o  7: 401ccec5e39f draft 'C'
  |
  o  5: 45396c49d53b public 'B'
  |
  o  4: ae36e8e3dfd7 public 'E'
  |
  o  3: 46b37eabc604 public 'D'
  |
  | o  1: 27547f69f254 public 'B'
  |/
  o  0: 4a2df7238c3b public 'A'
  
  $ cd ..

(pretxnclose version)

  $ cp -R a3 hook-pretxnclose
  $ cd hook-pretxnclose
  $ hg rebase --source 2 --dest 5 --tool internal:other --config 'hooks.pretxnclose=hg log -r tip | grep "summary:     C"'
  rebasing 965c486023db "C"
  summary:     C
  rebasing a0b2430ebfb8 "F"
  transaction abort!
  rollback completed
  abort: pretxnclose hook exited with status 1
  [255]
  $ tglogp
  @  7: 401ccec5e39f draft 'C'
  |
  | @  6: a0b2430ebfb8 draft 'F'
  | |
  o |  5: 45396c49d53b public 'B'
  | |
  o |  4: ae36e8e3dfd7 public 'E'
  | |
  o |  3: 46b37eabc604 public 'D'
  | |
  | x  2: 965c486023db draft 'C'
  | |
  | o  1: 27547f69f254 public 'B'
  |/
  o  0: 4a2df7238c3b public 'A'
  
  $ hg rebase --continue
  already rebased 965c486023db "C" as 401ccec5e39f
  rebasing a0b2430ebfb8 "F"
  $ tglogp
  @  8: 6e92a149ac6b draft 'F'
  |
  o  7: 401ccec5e39f draft 'C'
  |
  o  5: 45396c49d53b public 'B'
  |
  o  4: ae36e8e3dfd7 public 'E'
  |
  o  3: 46b37eabc604 public 'D'
  |
  | o  1: 27547f69f254 public 'B'
  |/
  o  0: 4a2df7238c3b public 'A'
  
  $ cd ..

Make sure merge state is cleaned up after a no-op rebase merge (issue5494)
  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg commit -qAm base
  $ echo b >> a
  $ hg commit -qm b
  $ hg up '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> a
  $ hg commit -qm c
  $ hg rebase -s 1 -d 2 --noninteractive
  rebasing fdaca8533b86 "b"
  merging a
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ echo a > a
  $ echo c >> a
  $ hg resolve --mark a
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing fdaca8533b86 "b"
  note: rebase of 1:fdaca8533b86 created no changes to commit
  $ hg resolve --list
  $ test -f .hg/merge
  [1]
