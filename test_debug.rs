use db_sim_core::character::*;

fn main() {
    match validate_roster() {
        Ok(()) => println!("Roster is valid"),
        Err(e) => println!("Roster error: {:?}", e),
    }
}
