-- Add migration script here
ALTER TABLE users ADD COLUMN address varchar not null default '';
ALTER TABLE users ADD COLUMN phone varchar not null default '';
ALTER TABLE users ADD COLUMN email varchar not null default '';
ALTER TABLE users ADD COLUMN rate_hourly_cents int not null default 0;
ALTER TABLE users ADD COLUMN rate_mileage_cents int not null default 0;
ALTER TABLE users ADD COLUMN rate_drive_hourly_cents int not null default 0;