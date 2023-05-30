-- Add migration script here
ALTER TABLE users
    ADD COLUMN flat_rate_cents int not null default 0;