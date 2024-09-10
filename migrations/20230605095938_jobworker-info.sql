-- Add migration script here
ALTER TABLE jobworkers ADD COLUMN miles_driven real not null default 0;
ALTER TABLE jobworkers ADD COLUMN hours_driven real not null default 0;