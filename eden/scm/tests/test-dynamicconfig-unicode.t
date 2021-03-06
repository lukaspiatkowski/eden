#chg-compatible

  $ configure modern

  $ setconfig configs.loaddynamicconfig=True
  $ export HG_TEST_DYNAMICCONFIG="$TESTTMP/test_hgrc"
  $ cat > test_hgrc <<EOF
  > [section]
  > key=✓
  > EOF

  $ hg init client
  $ cd client

Verify it can be manually generated

  $ hg debugdynamicconfig
  $ cat .hg/hgrc.dynamic
  # version=4.4.2* (glob)
  # Generated by `hg debugdynamicconfig` - DO NOT MODIFY
  [section]
  key=✓
  
  $ hg config section.key
  ✓
