use image::{GenericImageView, imageops, Rgba};
use std::env;
use std::path::Path;

// Function to display pixel art in console with emoji square pixels
fn display_pixel_art(binary_grid: &Vec<Vec<u8>>) {
    println!("\nPixel Art Representation:");
    
    // Display the grid with emoji squares
    for row in binary_grid {
        for &cell in row {
            // Use black square emoji for filled pixels, white square for empty
            print!("{}", if cell == 1 { "⬛" } else { "⬜" });
        }
        println!();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get command line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run --bin img2hex <image_path>");
        return Ok(());
    }

    let image_path = &args[1];
    println!("Processing image: {}", image_path);

    // Load the image
    let img = image::open(Path::new(image_path))?;
    
    // Resize to 50x50
    let resized = imageops::resize(&img, 50, 50, imageops::FilterType::Lanczos3);
    
    // Convert to black and white image
    let mut binary_grid = vec![vec![0; 50]; 50];
    
    // Threshold for determining black/white boundary (0-255)
    let threshold = 128;
    
    // Iterate through pixels and convert to binary grid
    for y in 0..50 {
        for x in 0..50 {
            let pixel = resized.get_pixel(x, y);
            
            // Calculate grayscale value (simple average method)
            let gray_value = (pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32) / 3;
            
            // Determine if black(1) or white(0) based on threshold
            binary_grid[y as usize][x as usize] = if gray_value < threshold { 1 } else { 0 };
        }
    }
    
    // Generate hexadecimal string
    let mut hex_string = String::new();
    
    for row in &binary_grid {
        for chunk_start in (0..row.len()).step_by(4) {
            let end = std::cmp::min(chunk_start + 4, row.len());
            let chunk = &row[chunk_start..end];
            
            // Convert 4 binary bits to hexadecimal
            let mut value = 0;
            for (i, &bit) in chunk.iter().enumerate() {
                value |= bit << (3 - i);
            }
            
            hex_string.push_str(&format!("{:X}", value));
        }
    }
    
    // Output results
    println!("Hex string representation:");
    println!("{}", hex_string);
    
    // Display the pixel art in console
    display_pixel_art(&binary_grid);
    
    // Optional: Save the processed black and white image
    let mut output = image::RgbaImage::new(50, 50);
    for y in 0..50 {
        for x in 0..50 {
            let color = if binary_grid[y as usize][x as usize] == 1 {
                Rgba([0, 0, 0, 255])  // Black
            } else {
                Rgba([255, 255, 255, 255])  // White
            };
            output.put_pixel(x, y, color);
        }
    }
    
    let output_path = format!("{}_processed.png", image_path);
    output.save(&output_path)?;
    println!("Processed image saved to: {}", output_path);
    
    // Print pixel: prefix for easy copying
    println!("\nFor use with mint command:");
    println!("pixel:{}", hex_string);
    
    Ok(())
} 