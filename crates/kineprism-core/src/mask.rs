use crate::{Bounds, CompareError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Component {
    pub(crate) bounds: Bounds,
    pub(crate) area: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mask {
    width: u32,
    height: u32,
    values: Vec<bool>,
}

impl Mask {
    pub(crate) fn try_new(width: u32, height: u32) -> Result<Self, CompareError> {
        let area = usize::try_from(u64::from(width) * u64::from(height))
            .map_err(|_error| CompareError::ImageTooLarge)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(area)
            .map_err(|_error| CompareError::ImageTooLarge)?;
        values.resize(area, false);
        Ok(Self {
            width,
            height,
            values,
        })
    }

    pub(crate) fn set(&mut self, x: u32, y: u32, value: bool) {
        let index = self.index(x, y);
        self.values[index] = value;
    }

    pub(crate) fn components(&self, minimum_area: u32) -> Result<Vec<Component>, CompareError> {
        let mut visited = Vec::new();
        visited
            .try_reserve_exact(self.values.len())
            .map_err(|_error| CompareError::ImageTooLarge)?;
        visited.resize(self.values.len(), false);
        let mut stack = Vec::new();
        stack
            .try_reserve_exact(self.values.len())
            .map_err(|_error| CompareError::ImageTooLarge)?;
        let mut components = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                let index = self.index(x, y);
                if visited[index] || !self.values[index] {
                    continue;
                }
                let component = self.flood_component(x, y, &mut visited, &mut stack);
                if component.area >= minimum_area {
                    components
                        .try_reserve(1)
                        .map_err(|_error| CompareError::ImageTooLarge)?;
                    components.push(component);
                }
            }
        }
        Ok(components)
    }

    fn flood_component(
        &self,
        start_x: u32,
        start_y: u32,
        visited: &mut [bool],
        stack: &mut Vec<(u32, u32)>,
    ) -> Component {
        stack.clear();
        stack.push((start_x, start_y));
        let mut area = 0_u32;
        let mut minimum_x = start_x;
        let mut minimum_y = start_y;
        let mut maximum_x = start_x;
        let mut maximum_y = start_y;
        visited[self.index(start_x, start_y)] = true;

        while let Some((x, y)) = stack.pop() {
            area = area.saturating_add(1);
            minimum_x = minimum_x.min(x);
            minimum_y = minimum_y.min(y);
            maximum_x = maximum_x.max(x);
            maximum_y = maximum_y.max(y);
            for neighbor_y in y.saturating_sub(1)..=(y + 1).min(self.height - 1) {
                for neighbor_x in x.saturating_sub(1)..=(x + 1).min(self.width - 1) {
                    let index = self.index(neighbor_x, neighbor_y);
                    if !visited[index] && self.values[index] {
                        visited[index] = true;
                        stack.push((neighbor_x, neighbor_y));
                    }
                }
            }
        }

        Component {
            bounds: Bounds {
                x: minimum_x,
                y: minimum_y,
                width: maximum_x - minimum_x + 1,
                height: maximum_y - minimum_y + 1,
            },
            area,
        }
    }

    fn index(&self, x: u32, y: u32) -> usize {
        assert!(
            x < self.width && y < self.height,
            "mask coordinate out of bounds"
        );
        usize::try_from(u64::from(y) * u64::from(self.width) + u64::from(x))
            .expect("validated mask index")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_use_eight_neighbor_connectivity_and_scan_order() {
        let mut mask = Mask::try_new(5, 4).expect("mask");
        mask.set(3, 0, true);
        mask.set(0, 1, true);
        mask.set(1, 2, true);

        let components = mask.components(1).expect("components");

        assert_eq!(components.len(), 2);
        assert_eq!(components[0].bounds.x, 3);
        assert_eq!(components[1].area, 2);
        assert_eq!(components[1].bounds.width, 2);
        assert_eq!(components[1].bounds.height, 2);
    }

    #[test]
    fn components_below_the_minimum_area_are_filtered() {
        let mut mask = Mask::try_new(2, 1).expect("mask");
        mask.set(0, 0, true);
        assert!(mask.components(2).expect("components").is_empty());
    }
}
