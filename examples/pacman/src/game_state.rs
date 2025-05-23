pub const MOVE_RIGHT: i8 = 1;
pub const MOVE_LEFT: i8 = -1;

#[derive(Clone)]
pub struct GameState {
    pub pacman_position: u8,
    pub food_positions: Vec<bool>,
    pub direction: i8,
    pub pacman_eating: bool,
    previous_pacman_position: u8,
    previous_pacman_eating: bool,
    previous_direction: i8,
}

impl GameState {
    pub fn new(display_key_count: u8) -> Self {
        let mut food_positions = vec![false; display_key_count as usize];

        // Fill all positions except first (where pacman starts) with food
        for i in 1..display_key_count {
            food_positions[i as usize] = true;
        }

        Self {
            pacman_position: 0,
            food_positions,
            direction: MOVE_RIGHT,
            pacman_eating: false,
            previous_pacman_position: 0,
            previous_pacman_eating: false,
            previous_direction: MOVE_RIGHT,
        }
    }

    pub fn move_pacman(&mut self, display_key_count: u8) {
        self.previous_pacman_position = self.pacman_position;

        self.pacman_position = match self.direction {
            MOVE_RIGHT => {
                if self.pacman_position == display_key_count - 1 {
                    0 // Wrap around
                } else {
                    self.pacman_position + 1
                }
            }
            MOVE_LEFT => {
                if self.pacman_position == 0 {
                    display_key_count - 1 // Wrap around
                } else {
                    self.pacman_position - 1
                }
            }
            _ => self.pacman_position, // Invalid direction, stay in place
        };
    }

    pub fn set_direction(&mut self, direction: i8) {
        self.previous_direction = self.direction;
        self.direction = direction;
    }

    pub fn set_eating_state(&mut self, eating: bool) {
        self.previous_pacman_eating = self.pacman_eating;
        self.pacman_eating = eating;
    }

    pub fn add_food(&mut self, position: u8) {
        if let Some(pos) = self.food_positions.get_mut(position as usize) {
            *pos = true;
        }
    }

    pub fn eat_food(&mut self, position: u8) -> bool {
        if let Some(pos) = self.food_positions.get_mut(position as usize) {
            if *pos {
                *pos = false;
                return true;
            }
        }
        false
    }

    pub fn has_food(&self, position: u8) -> bool {
        self.food_positions.get(position as usize).copied().unwrap_or(false)
    }

    pub fn has_state_changed(&self) -> bool {
        self.previous_pacman_position != self.pacman_position
            || self.previous_pacman_eating != self.pacman_eating
            || self.previous_direction != self.direction
    }

    pub fn position_changed(&self) -> bool {
        self.previous_pacman_position != self.pacman_position
    }

    pub fn get_previous_position(&self) -> u8 {
        self.previous_pacman_position
    }
}
