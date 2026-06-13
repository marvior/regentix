import { bootstrapApplication } from '@angular/platform-browser';
import { Component, Injectable } from '@angular/core';
import { HttpClient, provideHttpClient } from '@angular/common/http';
import { appConfig } from './app/app.config';
import { CommonModule } from '@angular/common';
import { Observable } from 'rxjs';

// 1. IMPORTA IL COMPONENTE DALLA SUA SOTTODIRECTORY
import { RegoTableComponent } from './app/rego-table/rego-table';
interface RegoRow {
  metodo: string;
  parametri: any[];
  userPrompt: string;
  aiResult: string;
  isLoading: boolean;
}

// -------------------------------------------------------------
// 1. IL SERVIZIO PER PARLARE CON IL SERVER BACKEND MLX
// -------------------------------------------------------------
@Injectable({
  providedIn: 'root'
})
export class MlxServerService {
  constructor(private http: HttpClient) {}

  inviaPrompt(prompt: string) {
        

    const body = {
      model: '',
      messages: [{ role: 'user', content: prompt }]    
    };
    return this.http.post<any>('/api/v1/chat/completions', body);
  }

  getMethodsConfig(): Observable<any[]> {
    return this.http.get<any[]>('/api/v1/execute-config');
  }

  salvaPolicy(serverName:string,methodName: string, regoCode: string): Observable<any> {
    const body = {
      server_name:serverName,
      method_name: methodName,
      rego_code: regoCode
    };
    return this.http.post<any>('/api/v1/save-policy', body);
  }
}



// (La vecchia sezione 3 di RegoTableComponent inline è stata rimossa con successo)

// -------------------------------------------------------------
// 4. IL COMPONENTE ROOT PRINCIPALE 
// -------------------------------------------------------------
@Component({
  selector: 'app-root',
  standalone: true,
  imports: [CommonModule, RegoTableComponent], // Utilizza l'import esterno
  template: `
    <div style="padding: 20px; font-family: system-ui, sans-serif; max-width: 1100px; margin: 0 auto;">
      <h1 style="margin: 0 0 5px 0; color: #333;">🧠 Regentix LLM Interface</h1>
      <p style="color: #666; font-size: 14px; margin: 0 0 20px 0;">Local Model: <strong>Qwen/Qwen2.5-Coder-1.5B-Instruct (Fine-tuned LoRA)</strong></p>
      <div style="display: flex; gap: 10px; margin-bottom: 20px; border-bottom: 2px solid #eee; padding-bottom: 10px;">
        <button (click)="schedaAttiva = 'tabella'" [style.background]="schedaAttiva === 'tabella' ? '#007bff' : '#6c757d'" style="color: white; padding: 10px 20px; border: none; border-radius: 4px; cursor: pointer; font-weight: bold; transition: background 0.2s;">
          📋 Manage Rules
        </button>
      </div>

      <app-rego-table *ngIf="schedaAttiva === 'tabella'"></app-rego-table>
    </div>
  `,
})
export class App {
  schedaAttiva = 'chat';
}

bootstrapApplication(App, {
  providers: [
    ...appConfig.providers || [],
    provideHttpClient() 
  ]
}).catch((err) => console.error(err));