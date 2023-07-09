-- Add migration script here
ALTER TABLE jobs
    ADD COLUMN notes varchar not null default '';