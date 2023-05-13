-- Add migration script here
ALTER TABLE users ADD CONSTRAINT unq_name UNIQUE (name);
