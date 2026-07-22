module database

import db.sqlite
import models

pub fn initialize(path string) !sqlite.DB {
	conn := sqlite.connect(path)!
	sql conn {
		create table models.User
		create table models.Pet
		create table models.Cookie
		create table models.CombiBonus
		create table models.Treasure
		create table models.Effect
		create table models.TreasureEffect
	}!
	return conn
}