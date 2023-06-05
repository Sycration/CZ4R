-- Add migration script here
ALTER TABLE jobworkers
    ADD COLUMN using_flat_rate boolean not null default false;