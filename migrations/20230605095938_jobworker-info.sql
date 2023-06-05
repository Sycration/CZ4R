-- Add migration script here
ALTER TABLE jobworkers
    ADD COLUMN miles_driven int not null default 0,
    ADD COLUMN hours_driven int not null default 0;