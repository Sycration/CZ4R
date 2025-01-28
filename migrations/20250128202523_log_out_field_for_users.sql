-- Add migration script here
alter table users add column logged_out boolean not null default 0;