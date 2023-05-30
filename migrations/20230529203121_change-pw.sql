-- Add migration script here
ALTER TABLE users
    ADD COLUMN must_change_pw boolean not null default false;