-- The product no longer keeps people's GitHub (docs/plans/usable/design-github-identity.zh.md 3.4, 4):
-- delete the connections (with their encrypted tokens), the sign-ins in progress and the runs that
-- waited for one. Rows only: deploy-server.sh applies migrations before it deploys the Worker, and
-- the old Worker still reads these tables in that gap and after a rollback. A later release drops
-- the tables (00NN_github_drop.sql).
DELETE FROM github_device;
DELETE FROM github_waits;
DELETE FROM github_accounts;
