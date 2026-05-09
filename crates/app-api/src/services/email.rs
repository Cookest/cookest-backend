use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};

#[derive(Debug, Serialize)]
struct ResendEmailRequest {
    from: String,
    to: String,
    subject: String,
    html: String,
}

#[derive(Debug, Deserialize)]
struct ResendEmailResponse {
    id: String,
}

pub struct EmailService {
    client: Arc<Client>,
    api_key: String,
    from_email: String,
}

impl EmailService {
    pub fn new(api_key: String, from_email: String) -> Self {
        Self {
            client: Arc::new(Client::new()),
            api_key,
            from_email,
        }
    }

    pub async fn send_verification_email(&self, to_email: String, token: String) -> Result<String, String> {
        if self.api_key.is_empty() {
            return Err("RESEND_API_KEY not configured".to_string());
        }

        let verification_link = format!("https://m.cookest.app/verify?token={}", token);
        
        let request = ResendEmailRequest {
            from: self.from_email.clone(),
            to: to_email.clone(),
            subject: "Verify your Cookest account".to_string(),
            html: self.verification_email_html(&verification_link),
        };

        match self.send_email(request).await {
            Ok(response_id) => {
                info!("Verification email sent to {}: {}", to_email, response_id);
                Ok(response_id)
            }
            Err(e) => {
                error!("Failed to send verification email: {}", e);
                Err(e)
            }
        }
    }

    pub async fn send_password_reset_email(&self, to_email: String, token: String) -> Result<String, String> {
        if self.api_key.is_empty() {
            return Err("RESEND_API_KEY not configured".to_string());
        }

        let reset_link = format!("https://m.cookest.app/reset-password?token={}", token);
        
        let request = ResendEmailRequest {
            from: self.from_email.clone(),
            to: to_email.clone(),
            subject: "Reset your Cookest password".to_string(),
            html: self.password_reset_email_html(&reset_link),
        };

        match self.send_email(request).await {
            Ok(response_id) => {
                info!("Password reset email sent to {}: {}", to_email, response_id);
                Ok(response_id)
            }
            Err(e) => {
                error!("Failed to send password reset email: {}", e);
                Err(e)
            }
        }
    }

    pub async fn send_welcome_email(&self, to_email: String, name: String) -> Result<String, String> {
        if self.api_key.is_empty() {
            return Err("RESEND_API_KEY not configured".to_string());
        }

        let request = ResendEmailRequest {
            from: self.from_email.clone(),
            to: to_email.clone(),
            subject: "Welcome to Cookest!".to_string(),
            html: self.welcome_email_html(&name),
        };

        match self.send_email(request).await {
            Ok(response_id) => {
                info!("Welcome email sent to {}: {}", to_email, response_id);
                Ok(response_id)
            }
            Err(e) => {
                error!("Failed to send welcome email: {}", e);
                Err(e)
            }
        }
    }

    async fn send_email(&self, request: ResendEmailRequest) -> Result<String, String> {
        match self.client
            .post("https://api.resend.com/emails")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
        {
            Ok(response) => {
                match response.json::<ResendEmailResponse>().await {
                    Ok(data) => Ok(data.id),
                    Err(e) => Err(format!("Failed to parse Resend response: {}", e)),
                }
            }
            Err(e) => Err(format!("Failed to send email via Resend: {}", e)),
        }
    }

    fn verification_email_html(&self, verification_link: &str) -> String {
        format!(
            r#"<!DOCTYPE html>
<html>
  <head>
    <meta charset="utf-8" />
    <style>
      body {{ font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif; }}
      .container {{ max-width: 600px; margin: 0 auto; padding: 20px; }}
      .header {{ text-align: center; margin-bottom: 30px; }}
      .content {{ background-color: #f9fafb; padding: 20px; border-radius: 8px; }}
      .button {{ 
        background-color: #10b981; 
        color: white; 
        padding: 12px 30px; 
        text-decoration: none; 
        border-radius: 6px; 
        display: inline-block; 
        margin: 20px 0;
      }}
      .footer {{ text-align: center; color: #666; font-size: 12px; margin-top: 20px; }}
    </style>
  </head>
  <body>
    <div class="container">
      <div class="header">
        <h1>Welcome to Cookest!</h1>
      </div>
      <div class="content">
        <p>Thank you for signing up. Please verify your email address to get started.</p>
        <p>
          <a href="{}" class="button">Verify Email</a>
        </p>
        <p>Or copy and paste this link in your browser:</p>
        <p style="word-break: break-all; color: #666; font-size: 14px;">{}</p>
      </div>
      <div class="footer">
        <p>© 2026 Cookest. All rights reserved.</p>
      </div>
    </div>
  </body>
</html>"#,
            verification_link, verification_link
        )
    }

    fn password_reset_email_html(&self, reset_link: &str) -> String {
        format!(
            r#"<!DOCTYPE html>
<html>
  <head>
    <meta charset="utf-8" />
    <style>
      body {{ font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif; }}
      .container {{ max-width: 600px; margin: 0 auto; padding: 20px; }}
      .header {{ text-align: center; margin-bottom: 30px; }}
      .content {{ background-color: #f9fafb; padding: 20px; border-radius: 8px; }}
      .button {{ 
        background-color: #10b981; 
        color: white; 
        padding: 12px 30px; 
        text-decoration: none; 
        border-radius: 6px; 
        display: inline-block; 
        margin: 20px 0;
      }}
      .footer {{ text-align: center; color: #666; font-size: 12px; margin-top: 20px; }}
      .warning {{ background-color: #fef3c7; padding: 10px; border-radius: 4px; margin: 10px 0; }}
    </style>
  </head>
  <body>
    <div class="container">
      <div class="header">
        <h1>Password Reset</h1>
      </div>
      <div class="content">
        <p>We received a request to reset your Cookest password. Click the button below to set a new password.</p>
        <p>
          <a href="{}" class="button">Reset Password</a>
        </p>
        <p>Or copy and paste this link in your browser:</p>
        <p style="word-break: break-all; color: #666; font-size: 14px;">{}</p>
        <div class="warning">
          <p><strong>Note:</strong> This link will expire in 24 hours. If you didn't request a password reset, please ignore this email.</p>
        </div>
      </div>
      <div class="footer">
        <p>© 2026 Cookest. All rights reserved.</p>
      </div>
    </div>
  </body>
</html>"#,
            reset_link, reset_link
        )
    }

    fn welcome_email_html(&self, name: &str) -> String {
        format!(
            r#"<!DOCTYPE html>
<html>
  <head>
    <meta charset="utf-8" />
    <style>
      body {{ font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif; }}
      .container {{ max-width: 600px; margin: 0 auto; padding: 20px; }}
      .header {{ text-align: center; margin-bottom: 30px; }}
      .content {{ background-color: #f9fafb; padding: 20px; border-radius: 8px; }}
      .button {{ 
        background-color: #10b981; 
        color: white; 
        padding: 12px 30px; 
        text-decoration: none; 
        border-radius: 6px; 
        display: inline-block; 
        margin: 20px 0;
      }}
      .footer {{ text-align: center; color: #666; font-size: 12px; margin-top: 20px; }}
      .features {{ margin: 20px 0; }}
      .feature {{ margin: 10px 0; padding-left: 20px; }}
    </style>
  </head>
  <body>
    <div class="container">
      <div class="header">
        <h1>Welcome, {}!</h1>
      </div>
      <div class="content">
        <p>We're excited to have you join Cookest. Start exploring intelligent meal planning, smart grocery lists, and personalized recipes.</p>
        <div class="features">
          <h3>Get started with:</h3>
          <div class="feature">✓ AI-powered meal planning</div>
          <div class="feature">✓ Inventory management</div>
          <div class="feature">✓ Shopping list optimization</div>
          <div class="feature">✓ Recipe discovery</div>
        </div>
        <p>
          <a href="https://m.cookest.app" class="button">Open Cookest</a>
        </p>
      </div>
      <div class="footer">
        <p>© 2026 Cookest. All rights reserved.</p>
      </div>
    </div>
  </body>
</html>"#,
            name
        )
    }
}

