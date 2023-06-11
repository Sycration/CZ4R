-- Add migration script here
alter table jobworkers
alter column hours_driven type real,
alter column miles_driven type real;