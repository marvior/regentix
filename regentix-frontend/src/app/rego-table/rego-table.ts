import { Component } from '@angular/core';
import { CommonModule } from '@angular/common'; 
import { FormsModule } from '@angular/forms';   
// Importa il servizio globale risalendo di due cartelle (da src/app/rego-table/ a src/)
import { MlxServerService } from '../../main'; 


interface ActiveModal {
  row: any;
  field: 'userPrompt' | 'aiResult';
  title: string;
}


interface RegoRow {
  server_name: string; // <-- Aggiunto il campo proveniente dal backend
  metodo: string;
  description: string;
  parametri: any[];
  userPrompt: string;
  aiResult: string;
  isLoading: boolean;
}

@Component({
  selector: 'app-rego-table',
  standalone: true, 
  imports: [CommonModule, FormsModule],
  templateUrl: './rego-table.html', 
  styleUrls: ['./rego-table.css']    
})


export class RegoTableComponent {

  
  rows: RegoRow[] = [];
  activeModal: any = null;

  
  constructor(private mlxService: MlxServerService) {
    this.caricaConfigurazione();
  }

  caricaConfigurazione() {
    this.mlxService.getMethodsConfig().subscribe({
      next: (res) => {
        this.rows = res.map(item => ({
          ...item, // Copia automaticamente server_name, metodo e parametri dal JSON del backend
          userPrompt: '',
          aiResult: item.aiResult || '', 
          isLoading: false
        }));
      },
      error: (err) => {
        console.error(err);
        alert(err.error?.detail || "Errore durante la generazione e il caricamento dei metodi dal server.");
      }
    });
  }

  eseguiAI(row: RegoRow, index: number) {
    if (!row.userPrompt.trim()) {
      alert("Write a prompt before!");
      return;
    }

    row.isLoading = true;
    row.aiResult = 'Generazione in corso...';
    const parametriStr = JSON.stringify(row.parametri);
    
    const promptFinale = `${row.userPrompt} . action:"${row.metodo}"    args:${parametriStr}`;
    this.mlxService.inviaPrompt(promptFinale).subscribe({
      next: (risposta) => {
        let testoEstratto = '';

        if (!risposta) {
          testoEstratto = "❌ Errore: Risposta vuota dal server.";
        } else if (risposta.choices && risposta.choices[0] && risposta.choices[0].message) {
          testoEstratto = risposta.choices[0].message.content;
        } else if (typeof risposta === 'string') {
          testoEstratto = risposta;
        } else if (risposta.response) {
          testoEstratto = risposta.response;
        } else {
          testoEstratto = JSON.stringify(risposta);
        }

        this.rows[index].aiResult = testoEstratto;
        this.rows[index].isLoading = false;
        this.rows = [...this.rows];
      },
      error: (err) => {
        console.error(err);
        this.rows[index].aiResult = "❌ Errore di connessione o timeout del server.";
        this.rows[index].isLoading = false;
        this.rows = [...this.rows];
      }
    });
  }

  salvaRego(row: RegoRow) {

    this.mlxService.salvaPolicy(row.server_name,row.metodo, row.aiResult).subscribe({
      next: (response) => {
        alert(response.message || "File Saved. Now Restard Claude!");
      },
      error: (err) => {
        console.error(err);
        alert("Error saving.");
      }
    });
  }
}