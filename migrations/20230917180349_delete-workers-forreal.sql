-- Add migration script here
ALTER TABLE users
    ADD COLUMN deactivated boolean not null default false;
