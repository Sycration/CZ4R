-- Add migration script here
ALTER TABLE jobworkers
    ADD COLUMN deactivated boolean not null default false;