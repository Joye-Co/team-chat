-- An invite for an existing member (sign in again, a second Mac, after reactivation): redeeming
-- it gives that member a new token instead of making a new member.
ALTER TABLE invites ADD COLUMN member_id TEXT;
