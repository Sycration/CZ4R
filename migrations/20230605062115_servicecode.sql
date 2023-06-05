-- Add migration script here
ALTER TABLE jobs
    ADD COLUMN servicecode varchar not null;